//! Turns a blenkernel `Mesh` into GPU-ready triangles, following the surface batch in
//! `draw/intern/draw_cache_impl_mesh.cc`: one GPU vertex per face corner, indexed by
//! `Mesh::corner_tris` grouped by material slot.

use std::ops::Range;

use blenkernel::mesh::SHARP_FACE;
use blenkernel::{AttrDomain, Attribute, AttributeData, Mesh};
use bytemuck::{Pod, Zeroable};

/// Vertex layout of the surface batch (`pos` and `nor` in Blender's extractors).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct CornerVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceGeometry {
    pub vertices: Vec<CornerVertex>,
    /// Triangle list indexing `vertices`, counter-clockwise from outside, grouped by
    /// material slot.
    pub indices: Vec<u32>,
    /// Each material slot's range in `indices`; at least one slot.
    pub slot_ranges: Vec<Range<u32>>,
}

/// Builds the surface batch. Sharp faces get their face normal, smooth faces the vertex
/// normals, as in `Mesh::normals_domain`.
pub fn extract_surface(mesh: &Mesh) -> SurfaceGeometry {
    let positions = mesh.positions();
    let corner_verts = mesh.corner_verts();
    let face_normals = mesh.face_normals();
    let vert_normals = mesh.vert_normals();
    let sharp_faces = sharp_faces(mesh);

    let mut vertices = Vec::with_capacity(mesh.corners_num());
    for face in 0..mesh.faces_num() {
        let sharp = sharp_faces.is_some_and(|sharp| sharp[face]);
        for corner in mesh.face(face) {
            let vert = corner_verts[corner] as usize;
            let normal = if sharp {
                face_normals[face]
            } else {
                vert_normals[vert]
            };
            vertices.push(CornerVertex {
                position: positions[vert].to_array(),
                normal: normal.to_array(),
            });
        }
    }
    let (indices, slot_ranges) = triangles_by_slot(mesh);
    SurfaceGeometry {
        vertices,
        indices,
        slot_ranges,
    }
}

/// Material slots a mesh draws with: one per slot, and one even without slots
/// (`mesh_render_mat_len_get`).
pub fn material_slot_count(mesh: &Mesh) -> usize {
    mesh.materials().len().max(1)
}

/// Triangle indices grouped by material slot, so each slot draws as one range (`SortedFaceData`
/// in `draw_cache_extract_mesh_render_data.cc`).
fn triangles_by_slot(mesh: &Mesh) -> (Vec<u32>, Vec<Range<u32>>) {
    let tri_slots = mesh.tri_material_indices();
    let mut slot_sizes = vec![0u32; material_slot_count(mesh)];
    for &slot in &tri_slots {
        slot_sizes[slot as usize] += 3;
    }
    let mut next_start = 0;
    let slot_ranges: Vec<Range<u32>> = slot_sizes
        .iter()
        .map(|&size| {
            let start = next_start;
            next_start += size;
            start..next_start
        })
        .collect();

    let mut cursors: Vec<u32> = slot_ranges.iter().map(|range| range.start).collect();
    let mut indices = vec![0; next_start as usize];
    for (tri, &slot) in mesh.corner_tris().iter().zip(&tri_slots) {
        let at = cursors[slot as usize] as usize;
        indices[at..at + 3].copy_from_slice(tri);
        cursors[slot as usize] += 3;
    }
    (indices, slot_ranges)
}

fn sharp_faces(mesh: &Mesh) -> Option<&[bool]> {
    match mesh.attributes().lookup(SHARP_FACE) {
        Some(Attribute {
            domain: AttrDomain::Face,
            data: AttributeData::Bool(values),
            ..
        }) => Some(values),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use blenkernel::{Main, primitives};
    use glam::Vec3;

    use super::*;

    const CUBE_FACES: usize = 6;

    fn default_cube() -> Mesh {
        primitives::cube(Vec3::splat(2.0))
    }

    #[test]
    fn cube_has_one_vertex_per_corner_and_two_triangles_per_face() {
        let surface = extract_surface(&default_cube());
        assert_eq!(surface.vertices.len(), CUBE_FACES * 4);
        assert_eq!(surface.indices.len(), CUBE_FACES * 2 * 3);
        assert!(
            surface
                .indices
                .iter()
                .all(|&i| (i as usize) < surface.vertices.len())
        );
    }

    #[test]
    fn mesh_without_material_slots_draws_as_one_range() {
        let surface = extract_surface(&default_cube());
        assert_eq!(surface.slot_ranges, vec![0..surface.indices.len() as u32]);
    }

    #[test]
    fn triangles_are_grouped_by_material_slot() {
        let mut main = Main::new();
        let mesh = main.mesh_add("Cube", default_cube());
        let slots = vec![
            Some(main.material_add("Red", false)),
            Some(main.material_add("Blue", false)),
        ];
        main.mesh_set_materials(mesh, slots).unwrap();
        main.mesh_mut(mesh)
            .unwrap()
            .set_material_indices(vec![0, 1, 0, 1, 1, 1])
            .unwrap();

        let cube = main.mesh(mesh).unwrap();
        let surface = extract_surface(cube);
        assert_eq!(surface.slot_ranges, vec![0..12, 12..36]);
        let face_of_corner = |corner: u32| {
            (0..cube.faces_num()).find(|&f| cube.face(f).contains(&(corner as usize)))
        };
        let slot_faces = |range: Range<u32>| -> Vec<usize> {
            surface.indices[range.start as usize..range.end as usize]
                .iter()
                .map(|&corner| face_of_corner(corner).unwrap())
                .collect()
        };
        assert!(slot_faces(0..12).iter().all(|face| [0, 2].contains(face)));
        assert!(
            slot_faces(12..36)
                .iter()
                .all(|face| [1, 3, 4, 5].contains(face))
        );
    }

    #[test]
    fn smooth_faces_use_vertex_normals() {
        let mut mesh = default_cube();
        mesh.set_smooth(true);
        let surface = extract_surface(&mesh);
        for (corner, vertex) in surface.vertices.iter().enumerate() {
            let vert = mesh.corner_verts()[corner] as usize;
            assert_eq!(vertex.normal, mesh.vert_normals()[vert].to_array());
        }
    }

    #[test]
    fn sharp_faces_use_face_normals() {
        let mesh = default_cube();
        let surface = extract_surface(&mesh);
        for face in 0..mesh.faces_num() {
            for corner in mesh.face(face) {
                assert_eq!(
                    surface.vertices[corner].normal,
                    mesh.face_normals()[face].to_array()
                );
            }
        }
    }
}
