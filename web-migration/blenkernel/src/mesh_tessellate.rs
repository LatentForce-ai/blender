//! Split faces into triangles (`blenkernel/intern/mesh_tessellate.cc`).
//!
//! Fan triangulation: matches Blender for triangles and quads ((0,1,2), (0,2,3)). Blender uses
//! polyfill for larger n-gons, so concave n-gons can triangulate differently here.

/// Triangles as triples of *corner* indices, like `Mesh::corner_tris()` in Blender.
pub fn calc_corner_tris(face_offsets: &[i32]) -> Vec<[u32; 3]> {
    let mut tris = Vec::new();
    for f in face_offsets.windows(2) {
        let (start, end) = (f[0] as u32, f[1] as u32);
        for i in (start + 1)..end.saturating_sub(1) {
            tris.push([start, i, i + 1]);
        }
    }
    tris
}
