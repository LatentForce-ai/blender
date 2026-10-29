//! Build the unique edges of a mesh from its faces (`blenkernel/intern/mesh_calc_edges.cc`).
//!
//! Edge *order* differs from Blender's hash-based implementation; the set of edges and the
//! `corner_edges` mapping are equivalent.

use std::collections::HashMap;

/// Returns `(edge_verts, corner_edges)`: every undirected edge once, and for each face corner the
/// edge from that corner to the next corner of the same face.
pub fn calc_edges(face_offsets: &[i32], corner_verts: &[i32]) -> (Vec<[i32; 2]>, Vec<i32>) {
    let mut edge_index: HashMap<(i32, i32), i32> = HashMap::new();
    let mut edge_verts: Vec<[i32; 2]> = Vec::new();
    let mut corner_edges = vec![0; corner_verts.len()];

    for face in face_offsets.windows(2) {
        let (start, end) = (face[0] as usize, face[1] as usize);
        for corner in start..end {
            let next = if corner + 1 == end { start } else { corner + 1 };
            let (a, b) = (corner_verts[corner], corner_verts[next]);
            let key = if a < b { (a, b) } else { (b, a) };
            let edge = *edge_index.entry(key).or_insert_with(|| {
                edge_verts.push([key.0, key.1]);
                (edge_verts.len() - 1) as i32
            });
            corner_edges[corner] = edge;
        }
    }

    (edge_verts, corner_edges)
}
