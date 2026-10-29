//! Mesh primitives (`geometry/intern/mesh_primitive_*.cc`).
//!
//! Vertex and face order follow Blender for the grid and UV sphere. The cube uses its own vertex
//! order (Blender's `create_cuboid_mesh` handles arbitrary subdivisions); counts, bounds and
//! outward winding match. Edges come from `calc_edges`, so edge order differs from Blender.
//! Invalid sizes are clamped to the smallest valid value instead of asserting.

use std::f32::consts::PI;

use glam::Vec3;

use crate::attribute::{AttrDomain, Attribute, AttributeData};
use crate::mesh::{Mesh, UV_MAP};

/// Builds a primitive shaded flat, as every `create_*_mesh` ends with
/// `mesh_smooth_set(mesh, false)`, with a `"UVMap"` (one UV per corner) as `calc_uvs` adds.
fn build(
    positions: Vec<Vec3>,
    face_sizes: impl IntoIterator<Item = i32>,
    corner_verts: Vec<i32>,
    uvs: Vec<[f32; 2]>,
) -> Mesh {
    let mut face_offsets = vec![0];
    for size in face_sizes {
        face_offsets.push(face_offsets.last().unwrap() + size);
    }
    let mut mesh =
        Mesh::from_faces(positions, face_offsets, corner_verts).expect("primitive topology is valid");
    mesh.set_smooth(false);
    mesh.add_attribute(Attribute {
        name: UV_MAP.into(),
        domain: AttrDomain::Corner,
        data: AttributeData::Float2(uvs),
    })
    .expect("one UV per corner");
    mesh
}

/// UVs of a quad's four corners covering the whole 0–1 square.
const QUAD_UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Box centered on the origin with 8 vertices and 6 quads
/// (`create_cuboid_mesh(size, 2, 2, 2)`). Blender's default cube is `size = 2`.
pub fn cube(size: Vec3) -> Mesh {
    let h = size * 0.5;
    // Vertex index bits: 1 = +X, 2 = +Y, 4 = +Z.
    let positions = (0..8)
        .map(|i| {
            Vec3::new(
                if i & 1 != 0 { h.x } else { -h.x },
                if i & 2 != 0 { h.y } else { -h.y },
                if i & 4 != 0 { h.z } else { -h.z },
            )
        })
        .collect();
    #[rustfmt::skip]
    let corner_verts = vec![
        0, 2, 3, 1, // -Z
        4, 5, 7, 6, // +Z
        0, 1, 5, 4, // -Y
        2, 6, 7, 3, // +Y
        0, 4, 6, 2, // -X
        1, 3, 7, 5, // +X
    ];
    // Each face gets the full image; Blender's cube unwraps into a cross instead.
    build(positions, [4; 6], corner_verts, QUAD_UVS.repeat(6))
}

/// Flat grid in the XY plane centered on the origin (`create_grid_mesh`).
/// `verts_x * verts_y` vertices, `(verts_x - 1) * (verts_y - 1)` quads.
pub fn grid(verts_x: u32, verts_y: u32, size_x: f32, size_y: f32) -> Mesh {
    let (verts_x, verts_y) = (verts_x.max(2) as i32, verts_y.max(2) as i32);
    let (edges_x, edges_y) = (verts_x - 1, verts_y - 1);
    let dx = size_x / edges_x as f32;
    let dy = size_y / edges_y as f32;
    let x_shift = edges_x as f32 / 2.0;
    let y_shift = edges_y as f32 / 2.0;

    // Columns along X, each column runs along Y: vertex = x * verts_y + y.
    let mut positions = Vec::with_capacity((verts_x * verts_y) as usize);
    for x in 0..verts_x {
        for y in 0..verts_y {
            positions.push(Vec3::new((x as f32 - x_shift) * dx, (y as f32 - y_shift) * dy, 0.0));
        }
    }

    let mut corner_verts = Vec::with_capacity((edges_x * edges_y * 4) as usize);
    let mut uvs = Vec::with_capacity(corner_verts.capacity());
    let uv = |x: i32, y: i32| [x as f32 / edges_x as f32, y as f32 / edges_y as f32];
    for x in 0..edges_x {
        for y in 0..edges_y {
            let v = x * verts_y + y;
            let v_next_x = (x + 1) * verts_y + y;
            corner_verts.extend([v, v_next_x, v_next_x + 1, v + 1]);
            uvs.extend([uv(x, y), uv(x + 1, y), uv(x + 1, y + 1), uv(x, y + 1)]);
        }
    }
    build(positions, std::iter::repeat(4).take((edges_x * edges_y) as usize), corner_verts, uvs)
}

/// UV sphere centered on the origin (`create_uv_sphere_mesh`). Blender's defaults are
/// 32 segments and 16 rings: 482 vertices, 512 faces (64 triangles at the poles, the rest quads).
pub fn uv_sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let (segments, rings) = (segments.max(3) as i32, rings.max(2) as i32);
    let delta_theta = PI / rings as f32;
    let delta_phi = 2.0 * PI / segments as f32;

    let mut positions = Vec::with_capacity((segments * (rings - 1) + 2) as usize);
    positions.push(Vec3::new(0.0, 0.0, radius));
    for ring in 1..rings {
        let theta = ring as f32 * delta_theta;
        let (sin_theta, z) = (theta.sin(), theta.cos());
        for segment in 1..=segments {
            let phi = segment as f32 * delta_phi;
            positions.push(Vec3::new(sin_theta * phi.cos(), sin_theta * phi.sin(), z) * radius);
        }
    }
    positions.push(Vec3::new(0.0, 0.0, -radius));
    let last_vert = positions.len() as i32 - 1;

    let next = |segment: i32| if segment == segments - 1 { 0 } else { segment + 1 };
    let mut corner_verts = Vec::new();
    let mut face_sizes = Vec::new();
    // Blender's sphere unwrap: u around the segments (the seam gets its own u = 1), v from the
    // bottom pole (0) to the top (1); pole corners sit mid-segment.
    let mut uvs = Vec::new();
    let uv = |segment: f32, ring: i32| [segment / segments as f32, 1.0 - ring as f32 / rings as f32];

    // Triangles around the top pole.
    for segment in 0..segments {
        corner_verts.extend([0, 1 + segment, 1 + next(segment)]);
        let s = segment as f32;
        uvs.extend([uv(s + 0.5, 0), uv(s, 1), uv(s + 1.0, 1)]);
        face_sizes.push(3);
    }
    // Quads between rings.
    for ring in 1..rings - 1 {
        let ring_start = 1 + (ring - 1) * segments;
        let next_ring_start = ring_start + segments;
        for segment in 0..segments {
            corner_verts.extend([
                ring_start + segment,
                next_ring_start + segment,
                next_ring_start + next(segment),
                ring_start + next(segment),
            ]);
            let s = segment as f32;
            uvs.extend([uv(s, ring), uv(s, ring + 1), uv(s + 1.0, ring + 1), uv(s + 1.0, ring)]);
            face_sizes.push(4);
        }
    }
    // Triangles around the bottom pole.
    let last_ring_start = last_vert - segments;
    for segment in 0..segments {
        corner_verts.extend([last_vert, last_ring_start + next(segment), last_ring_start + segment]);
        let s = segment as f32;
        uvs.extend([uv(s + 0.5, rings), uv(s + 1.0, rings - 1), uv(s, rings - 1)]);
        face_sizes.push(3);
    }

    build(positions, face_sizes, corner_verts, uvs)
}
