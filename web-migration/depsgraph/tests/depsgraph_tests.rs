//! Depsgraph checks: change one thing, confirm exactly the right objects and meshes are
//! recomputed, and that the evaluated matrices match blenkernel's own `object_world_matrix`.

use blenkernel::{primitives, CollectionRef, Main, MeshId, ObjectData, ObjectId, SceneId};
use depsgraph::{Depsgraph, Updates};
use glam::Vec3;

/// The demo scene from the blenkernel README: a cube, a UV sphere parented to it, a grid floor.
struct Demo {
    bmain: Main,
    scene: SceneId,
    cube: ObjectId,
    sphere: ObjectId,
    grid: ObjectId,
    cube_mesh: MeshId,
    sphere_mesh: MeshId,
    grid_mesh: MeshId,
}

fn demo() -> Demo {
    let mut bmain = Main::new();
    let scene = bmain.scene_add("Scene");
    let cube_mesh = bmain.mesh_add("Cube", primitives::cube(Vec3::splat(2.0)));
    let sphere_mesh = bmain.mesh_add("Sphere", primitives::uv_sphere(0.5, 32, 16));
    let grid_mesh = bmain.mesh_add("Grid", primitives::grid(10, 10, 10.0, 10.0));
    let cube = bmain.object_add("Cube", ObjectData::Mesh(cube_mesh)).unwrap();
    let sphere = bmain.object_add("Sphere", ObjectData::Mesh(sphere_mesh)).unwrap();
    let grid = bmain.object_add("Grid", ObjectData::Mesh(grid_mesh)).unwrap();
    for object in [cube, sphere, grid] {
        bmain.collection_link_object(CollectionRef::Scene(scene), object).unwrap();
    }
    bmain.object_set_parent(sphere, Some(cube)).unwrap();
    bmain.object_mut(sphere).unwrap().location = Vec3::new(0.0, 0.0, 2.0);
    bmain.object_mut(grid).unwrap().location = Vec3::new(0.0, 0.0, -1.0);
    Demo { bmain, scene, cube, sphere, grid, cube_mesh, sphere_mesh, grid_mesh }
}

/// A demo scene already evaluated once, so the next evaluation only sees new edits.
fn evaluated_demo() -> (Demo, Depsgraph) {
    let mut d = demo();
    let mut graph = Depsgraph::new(d.scene);
    graph.evaluate(&mut d.bmain).unwrap();
    (d, graph)
}

fn assert_matrices_match(graph: &Depsgraph, bmain: &Main, objects: &[ObjectId]) {
    for &object in objects {
        let evaluated = graph.world_matrix(object).expect("object is in the graph");
        let expected = bmain.object_world_matrix(object).unwrap();
        assert!(evaluated.abs_diff_eq(expected, 1e-5), "{object:?}: {evaluated} != {expected}");
    }
}

#[test]
fn first_evaluation_computes_everything() {
    let mut d = demo();
    let mut graph = Depsgraph::new(d.scene);
    let updates = graph.evaluate(&mut d.bmain).unwrap();

    assert!(updates.relations);
    assert!(updates.removed.is_empty());
    assert_eq!(graph.objects(), &[d.cube, d.sphere, d.grid]);
    // The parent is evaluated before its child.
    assert_eq!(updates.transforms, vec![d.cube, d.sphere, d.grid]);
    assert_eq!(updates.geometry, vec![d.cube_mesh, d.sphere_mesh, d.grid_mesh]);
    assert_matrices_match(&graph, &d.bmain, &[d.cube, d.sphere, d.grid]);
}

#[test]
fn nothing_changed_means_nothing_recomputed() {
    let (mut d, mut graph) = evaluated_demo();
    assert!(graph.evaluate(&mut d.bmain).unwrap().is_empty());
}

#[test]
fn moving_a_parent_recomputes_its_children_only() {
    let (mut d, mut graph) = evaluated_demo();
    let grid_before = graph.world_matrix(d.grid).unwrap();

    d.bmain.object_mut(d.cube).unwrap().rotation_euler.z = std::f32::consts::FRAC_PI_2;
    let updates = graph.evaluate(&mut d.bmain).unwrap();

    assert_eq!(updates, Updates { transforms: vec![d.cube, d.sphere], ..Updates::default() });
    assert_matrices_match(&graph, &d.bmain, &[d.cube, d.sphere]);
    assert_eq!(graph.world_matrix(d.grid).unwrap(), grid_before);
}

#[test]
fn moving_a_child_leaves_its_parent_alone() {
    let (mut d, mut graph) = evaluated_demo();
    d.bmain.object_mut(d.sphere).unwrap().scale = Vec3::splat(3.0);
    let updates = graph.evaluate(&mut d.bmain).unwrap();
    assert_eq!(updates.transforms, vec![d.sphere]);
    assert_matrices_match(&graph, &d.bmain, &[d.sphere]);
}

#[test]
fn editing_a_mesh_recomputes_geometry_not_transforms() {
    let (mut d, mut graph) = evaluated_demo();
    d.bmain.mesh_mut(d.grid_mesh).unwrap().positions_mut()[0].z = 1.0;
    let updates = graph.evaluate(&mut d.bmain).unwrap();
    assert_eq!(updates, Updates { geometry: vec![d.grid_mesh], ..Updates::default() });
}

#[test]
fn a_shared_mesh_is_recomputed_once() {
    let (mut d, mut graph) = evaluated_demo();
    let copy = d.bmain.object_add("Cube", ObjectData::Mesh(d.cube_mesh)).unwrap();
    d.bmain.collection_link_object(CollectionRef::Scene(d.scene), copy).unwrap();
    graph.evaluate(&mut d.bmain).unwrap();

    d.bmain.mesh_mut(d.cube_mesh).unwrap().positions_mut()[0].x = -2.0;
    let updates = graph.evaluate(&mut d.bmain).unwrap();
    assert_eq!(updates.geometry, vec![d.cube_mesh]);
}

#[test]
fn reparenting_rebuilds_the_graph() {
    let (mut d, mut graph) = evaluated_demo();
    d.bmain.object_set_parent(d.grid, Some(d.sphere)).unwrap();
    let updates = graph.evaluate(&mut d.bmain).unwrap();
    assert!(updates.relations);
    assert_eq!(updates.transforms, vec![d.grid]);
    assert_matrices_match(&graph, &d.bmain, &[d.grid]);

    // The grid now follows the cube through the sphere.
    d.bmain.object_mut(d.cube).unwrap().location.x = 5.0;
    let updates = graph.evaluate(&mut d.bmain).unwrap();
    assert!(!updates.relations);
    assert_eq!(updates.transforms, vec![d.cube, d.sphere, d.grid]);
    assert_matrices_match(&graph, &d.bmain, &[d.cube, d.sphere, d.grid]);
}

#[test]
fn removing_an_object_reports_it_and_frees_its_children() {
    let (mut d, mut graph) = evaluated_demo();
    d.bmain.object_remove(d.cube).unwrap();
    let updates = graph.evaluate(&mut d.bmain).unwrap();

    assert!(updates.relations);
    assert_eq!(updates.removed, vec![d.cube]);
    assert_eq!(graph.objects(), &[d.sphere, d.grid]);
    assert_eq!(graph.world_matrix(d.cube), None);
    // The sphere lost its parent, so its matrix is its own local one now.
    assert_eq!(updates.transforms, vec![d.sphere]);
    assert_matrices_match(&graph, &d.bmain, &[d.sphere]);
}

#[test]
fn linking_an_object_evaluates_it_and_its_mesh() {
    let (mut d, mut graph) = evaluated_demo();
    let floor = d.bmain.mesh_add("Floor", primitives::grid(2, 2, 1.0, 1.0));
    let extra = d.bmain.object_add("Floor", ObjectData::Mesh(floor)).unwrap();
    d.bmain.object_mut(extra).unwrap().location.y = 3.0;
    d.bmain.collection_link_object(CollectionRef::Scene(d.scene), extra).unwrap();
    let updates = graph.evaluate(&mut d.bmain).unwrap();

    assert!(updates.relations);
    assert_eq!(updates.transforms, vec![extra]);
    assert_eq!(updates.geometry, vec![floor]);
    assert_matrices_match(&graph, &d.bmain, &[extra]);
}

#[test]
fn objects_in_nested_collections_are_in_the_graph() {
    let (mut d, mut graph) = evaluated_demo();
    let props = d.bmain.collection_add("Props");
    d.bmain.collection_link_child(CollectionRef::Scene(d.scene), props).unwrap();
    let lamp = d.bmain.object_add("Lamp", ObjectData::Empty).unwrap();
    d.bmain.collection_link_object(CollectionRef::Collection(props), lamp).unwrap();
    let updates = graph.evaluate(&mut d.bmain).unwrap();

    assert!(graph.objects().contains(&lamp));
    assert_eq!(updates.transforms, vec![lamp]);
    assert!(updates.geometry.is_empty());
}

#[test]
fn a_parent_outside_the_scene_still_moves_its_child() {
    let (mut d, mut graph) = evaluated_demo();
    let handle = d.bmain.object_add("Handle", ObjectData::Empty).unwrap();
    d.bmain.object_set_parent(d.grid, Some(handle)).unwrap();
    graph.evaluate(&mut d.bmain).unwrap();
    assert!(!graph.objects().contains(&handle));

    d.bmain.object_mut(handle).unwrap().location.z = 4.0;
    let updates = graph.evaluate(&mut d.bmain).unwrap();
    assert_eq!(updates.transforms, vec![handle, d.grid]);
    assert_matrices_match(&graph, &d.bmain, &[d.grid]);
}

#[test]
fn objects_outside_the_scene_are_ignored() {
    let (mut d, mut graph) = evaluated_demo();
    let loose = d.bmain.object_add("Loose", ObjectData::Empty).unwrap();
    graph.evaluate(&mut d.bmain).unwrap();

    d.bmain.object_mut(loose).unwrap().location.x = 1.0;
    assert!(graph.evaluate(&mut d.bmain).unwrap().is_empty());
    assert_eq!(graph.world_matrix(loose), None);
}
