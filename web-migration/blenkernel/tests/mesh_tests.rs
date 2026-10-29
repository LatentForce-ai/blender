//! Mesh checks against the numbers Blender produces (PLAN.md §7).

use blenkernel::attribute::{AttrDomain, Attribute, AttributeData};
use blenkernel::{primitives, BkeError, Mesh};
use glam::Vec3;

const EPS: f32 = 1e-5;

fn counts(mesh: &Mesh) -> [usize; 4] {
    [mesh.verts_num(), mesh.edges_num(), mesh.faces_num(), mesh.corners_num()]
}

/// Every face normal points away from the mesh center (true for convex primitives).
fn assert_outward(mesh: &Mesh) {
    for face in 0..mesh.faces_num() {
        let verts = mesh.face_verts(face);
        let center: Vec3 = verts.iter().map(|&v| mesh.positions()[v as usize]).sum::<Vec3>()
            / verts.len() as f32;
        assert!(mesh.face_normals()[face].dot(center) > 0.0, "face {face} points inward");
    }
}

#[test]
fn cube_matches_blender_default_cube() {
    let cube = primitives::cube(Vec3::splat(2.0));
    assert_eq!(counts(&cube), [8, 12, 6, 24]);
    cube.validate().unwrap();

    let bounds = cube.bounds().unwrap();
    assert!(bounds.min.abs_diff_eq(Vec3::splat(-1.0), EPS));
    assert!(bounds.max.abs_diff_eq(Vec3::splat(1.0), EPS));

    let expected = [Vec3::NEG_Z, Vec3::Z, Vec3::NEG_Y, Vec3::Y, Vec3::NEG_X, Vec3::X];
    for (normal, expected) in cube.face_normals().iter().zip(expected) {
        assert!(normal.abs_diff_eq(expected, EPS), "{normal} != {expected}");
    }
    assert_outward(&cube);
}

#[test]
fn cube_vertex_normals_point_along_diagonals() {
    let cube = primitives::cube(Vec3::splat(2.0));
    for (position, normal) in cube.positions().iter().zip(cube.vert_normals()) {
        assert!(normal.abs_diff_eq(position.normalize(), EPS));
    }
}

#[test]
fn cube_triangles() {
    let cube = primitives::cube(Vec3::splat(2.0));
    let tris = cube.tri_vert_indices();
    assert_eq!(tris.len(), 12 * 3);
    assert!(tris.iter().all(|&v| v < 8));
}

#[test]
fn uv_sphere_matches_blender_defaults() {
    let sphere = primitives::uv_sphere(1.0, 32, 16);
    // Blender: segments * (rings - 1) + 2 verts, segments * (2 * rings - 1) edges,
    // segments * rings faces, 4 * segments * (rings - 2) + 6 * segments corners.
    assert_eq!(counts(&sphere), [482, 992, 512, 1984]);
    sphere.validate().unwrap();
    assert!(sphere.positions().iter().all(|p| (p.length() - 1.0).abs() < EPS));
    assert!(sphere.positions()[0].abs_diff_eq(Vec3::Z, EPS));
    assert!(sphere.positions()[481].abs_diff_eq(Vec3::NEG_Z, EPS));
    assert_outward(&sphere);
    // Triangles: 64 pole triangles + 448 quads * 2.
    assert_eq!(sphere.corner_tris().len(), 64 + 448 * 2);
}

#[test]
fn grid_matches_blender() {
    let grid = primitives::grid(10, 10, 8.0, 8.0);
    // Blender: edges = (x-1)*y + (y-1)*x.
    assert_eq!(counts(&grid), [100, 180, 81, 324]);
    grid.validate().unwrap();
    assert!(grid.face_normals().iter().all(|n| n.abs_diff_eq(Vec3::Z, EPS)));
    let bounds = grid.bounds().unwrap();
    assert!(bounds.min.abs_diff_eq(Vec3::new(-4.0, -4.0, 0.0), EPS));
    assert!(bounds.max.abs_diff_eq(Vec3::new(4.0, 4.0, 0.0), EPS));
    // Blender's vertex order: x-major columns.
    assert!(grid.positions()[1].abs_diff_eq(Vec3::new(-4.0, -4.0 + 8.0 / 9.0, 0.0), EPS));
}

#[test]
fn caches_update_after_moving_vertices() {
    let mut cube = primitives::cube(Vec3::splat(2.0));
    assert!(cube.bounds().unwrap().max.abs_diff_eq(Vec3::ONE, EPS));
    for p in cube.positions_mut() {
        *p *= 2.0;
    }
    assert!(cube.bounds().unwrap().max.abs_diff_eq(Vec3::splat(2.0), EPS));
}

#[test]
fn from_faces_rejects_bad_topology() {
    let tri = vec![Vec3::ZERO, Vec3::X, Vec3::Y];
    assert!(Mesh::from_faces(tri.clone(), vec![0, 3], vec![0, 1, 2]).is_ok());
    assert!(matches!(
        Mesh::from_faces(tri.clone(), vec![0, 3], vec![0, 1, 7]),
        Err(BkeError::InvalidTopology(_))
    ));
    assert!(matches!(
        Mesh::from_faces(tri.clone(), vec![0, 2], vec![0, 1]),
        Err(BkeError::InvalidTopology(_))
    ));
    assert!(matches!(
        Mesh::from_faces(tri, vec![1, 3], vec![0, 1, 2]),
        Err(BkeError::InvalidTopology(_))
    ));
}

#[test]
fn set_positions_checks_length() {
    let mut cube = primitives::cube(Vec3::ONE);
    assert!(matches!(
        cube.set_positions(vec![Vec3::ZERO]),
        Err(BkeError::LengthMismatch { expected: 8, got: 1, .. })
    ));
}

#[test]
fn attributes() {
    let mut cube = primitives::cube(Vec3::ONE);
    let weights = Attribute {
        name: "weight".into(),
        domain: AttrDomain::Point,
        data: AttributeData::Float(vec![0.5; 8]),
    };
    cube.add_attribute(weights.clone()).unwrap();
    assert_eq!(cube.attributes().lookup("weight"), Some(&weights));
    assert!(cube.add_attribute(weights).is_err(), "duplicate name");

    let reserved = Attribute {
        name: "position".into(),
        domain: AttrDomain::Point,
        data: AttributeData::Float3(vec![[0.0; 3]; 8]),
    };
    assert!(matches!(cube.add_attribute(reserved), Err(BkeError::AttributeNameTaken(_))));

    let wrong_length = Attribute {
        name: "uv".into(),
        domain: AttrDomain::Corner,
        data: AttributeData::Float2(vec![[0.0; 2]; 3]),
    };
    assert!(matches!(cube.add_attribute(wrong_length), Err(BkeError::LengthMismatch { .. })));
    assert!(cube.remove_attribute("weight"));
}
