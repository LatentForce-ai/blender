//! Face and vertex normals (`blenkernel/intern/mesh_normals.cc`).

use glam::Vec3;

/// Normal of one face, from its vertex indices (`face_normal_calc`).
///
/// Uses Newell's method for every face size. Blender special-cases triangles and quads, which
/// gives the same result for planar faces. Degenerate faces get +Z.
pub fn face_normal_calc(positions: &[Vec3], face_verts: &[i32]) -> Vec3 {
    let Some(&last) = face_verts.last() else {
        return Vec3::Z;
    };
    // Relative to the first vertex for precision far from the origin.
    let origin = positions[face_verts[0] as usize];
    let mut prev = positions[last as usize] - origin;
    let mut normal = Vec3::ZERO;
    for &vert in face_verts {
        let cur = positions[vert as usize] - origin;
        normal += prev.cross(cur);
        prev = cur;
    }
    let normal = normal.normalize_or_zero();
    if normal == Vec3::ZERO {
        Vec3::Z
    } else {
        normal
    }
}

/// Normals of all faces (`normals_calc_faces`).
pub fn calc_face_normals(positions: &[Vec3], face_offsets: &[i32], corner_verts: &[i32]) -> Vec<Vec3> {
    face_offsets
        .windows(2)
        .map(|f| face_normal_calc(positions, &corner_verts[f[0] as usize..f[1] as usize]))
        .collect()
}

/// Vertex normals: face normals weighted by the angle of the face at that vertex
/// (`normals_calc_verts`). Vertices not used by any face fall back to their normalized position,
/// as in Blender.
pub fn calc_vert_normals(
    positions: &[Vec3],
    face_offsets: &[i32],
    corner_verts: &[i32],
    face_normals: &[Vec3],
) -> Vec<Vec3> {
    let mut accumulated = vec![Vec3::ZERO; positions.len()];

    for (face_index, f) in face_offsets.windows(2).enumerate() {
        let verts = &corner_verts[f[0] as usize..f[1] as usize];
        let size = verts.len();
        let face_normal = face_normals[face_index];
        for i in 0..size {
            let vert = verts[i] as usize;
            let prev = verts[(i + size - 1) % size] as usize;
            let next = verts[(i + 1) % size] as usize;
            let dir_prev = (positions[prev] - positions[vert]).normalize_or_zero();
            let dir_next = (positions[next] - positions[vert]).normalize_or_zero();
            let angle = dir_prev.dot(dir_next).clamp(-1.0, 1.0).acos();
            accumulated[vert] += face_normal * angle;
        }
    }

    accumulated
        .iter()
        .zip(positions)
        .map(|(normal, position)| {
            let normal = normal.normalize_or_zero();
            if normal == Vec3::ZERO {
                position.normalize_or_zero()
            } else {
                normal
            }
        })
        .collect()
}
