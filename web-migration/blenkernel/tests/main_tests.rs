//! `Main` database checks: names, user counts, collections, change tracking.

use blenkernel::{
    primitives, recalc, BkeError, Change, CollectionRef, IdType, Main, ObjectData,
};
use glam::Vec3;

#[test]
fn names_are_unique_per_type() {
    let mut bmain = Main::new();
    let a = bmain.object_add("Cube", ObjectData::Empty).unwrap();
    let b = bmain.object_add("Cube", ObjectData::Empty).unwrap();
    let c = bmain.object_add("Cube.001", ObjectData::Empty).unwrap();
    let d = bmain.object_add("", ObjectData::Empty).unwrap();
    assert_eq!(bmain.object(a).unwrap().id.name, "Cube");
    assert_eq!(bmain.object(b).unwrap().id.name, "Cube.001");
    assert_eq!(bmain.object(c).unwrap().id.name, "Cube.002");
    assert_eq!(bmain.object(d).unwrap().id.name, "Object");

    // A different type may reuse the name.
    let mesh = bmain.mesh_add("Cube", primitives::cube(Vec3::ONE));
    assert_eq!(bmain.mesh(mesh).unwrap().id.name, "Cube");

    // Removing frees the name.
    bmain.object_remove(a).unwrap();
    let e = bmain.object_add("Cube", ObjectData::Empty).unwrap();
    assert_eq!(bmain.object(e).unwrap().id.name, "Cube");
}

#[test]
fn long_names_are_truncated() {
    let mut bmain = Main::new();
    let long = "x".repeat(300);
    let a = bmain.object_add(&long, ObjectData::Empty).unwrap();
    let b = bmain.object_add(&long, ObjectData::Empty).unwrap();
    assert_eq!(bmain.object(a).unwrap().id.name.len(), 255);
    let name_b = &bmain.object(b).unwrap().id.name;
    assert_eq!(name_b.len(), 255);
    assert!(name_b.ends_with(".001"));
}

#[test]
fn mesh_user_counts() {
    let mut bmain = Main::new();
    let mesh = bmain.mesh_add("Cube", primitives::cube(Vec3::ONE));
    let a = bmain.object_add("A", ObjectData::Mesh(mesh)).unwrap();
    let b = bmain.object_add("B", ObjectData::Mesh(mesh)).unwrap();
    assert_eq!(bmain.mesh(mesh).unwrap().id.users, 2);

    assert_eq!(
        bmain.mesh_remove(mesh),
        Err(BkeError::StillInUse { id_type: IdType::Mesh, handle: mesh.0, users: 2 })
    );
    bmain.object_set_data(a, ObjectData::Empty).unwrap();
    bmain.object_remove(b).unwrap();
    assert_eq!(bmain.mesh(mesh).unwrap().id.users, 0);
    bmain.mesh_remove(mesh).unwrap();
    assert!(bmain.mesh(mesh).is_err());
}

#[test]
fn stale_handles_are_rejected() {
    let mut bmain = Main::new();
    let a = bmain.object_add("A", ObjectData::Empty).unwrap();
    bmain.object_remove(a).unwrap();
    let b = bmain.object_add("B", ObjectData::Empty).unwrap();
    assert_ne!(a, b, "handles are never reused");
    assert_eq!(
        bmain.object(a).map(|_| ()),
        Err(BkeError::InvalidHandle { id_type: IdType::Object, handle: a.0 })
    );
    let missing_mesh = blenkernel::MeshId(42);
    assert!(bmain.object_add("C", ObjectData::Mesh(missing_mesh)).is_err());
}

#[test]
fn scene_objects_flattens_nested_collections() {
    let mut bmain = Main::new();
    let scene = bmain.scene_add("Scene");
    let props = bmain.collection_add("Props");
    let small = bmain.collection_add("Small Props");
    let a = bmain.object_add("A", ObjectData::Empty).unwrap();
    let b = bmain.object_add("B", ObjectData::Empty).unwrap();
    let c = bmain.object_add("C", ObjectData::Empty).unwrap();

    bmain.collection_link_object(CollectionRef::Scene(scene), a).unwrap();
    bmain.collection_link_child(CollectionRef::Scene(scene), props).unwrap();
    bmain.collection_link_child(CollectionRef::Collection(props), small).unwrap();
    bmain.collection_link_object(CollectionRef::Collection(props), b).unwrap();
    bmain.collection_link_object(CollectionRef::Collection(small), c).unwrap();
    // Linked twice: listed once, two users.
    bmain.collection_link_object(CollectionRef::Collection(small), a).unwrap();
    assert!(!bmain.collection_link_object(CollectionRef::Scene(scene), a).unwrap());

    assert_eq!(bmain.scene_objects(scene).unwrap(), vec![a, b, c]);
    assert_eq!(bmain.object(a).unwrap().id.users, 2);

    bmain.object_remove(b).unwrap();
    assert_eq!(bmain.scene_objects(scene).unwrap(), vec![a, c]);

    bmain.collection_remove(small).unwrap();
    assert_eq!(bmain.scene_objects(scene).unwrap(), vec![a]);
    assert_eq!(bmain.object(c).unwrap().id.users, 0);
    assert_eq!(bmain.object(a).unwrap().id.users, 1);
}

#[test]
fn collection_cycles_are_rejected() {
    let mut bmain = Main::new();
    let a = bmain.collection_add("A");
    let b = bmain.collection_add("B");
    bmain.collection_link_child(CollectionRef::Collection(a), b).unwrap();
    assert_eq!(
        bmain.collection_link_child(CollectionRef::Collection(b), a),
        Err(BkeError::CollectionCycle)
    );
    assert_eq!(
        bmain.collection_link_child(CollectionRef::Collection(a), a),
        Err(BkeError::CollectionCycle)
    );
}

#[test]
fn take_changes_reports_and_clears() {
    let mut bmain = Main::new();
    let scene = bmain.scene_add("Scene");
    let mesh = bmain.mesh_add("Cube", primitives::cube(Vec3::ONE));
    let cube = bmain.object_add("Cube", ObjectData::Mesh(mesh)).unwrap();
    bmain.collection_link_object(CollectionRef::Scene(scene), cube).unwrap();

    // Everything new is reported once with all flags.
    let first = bmain.take_changes();
    assert_eq!(first.len(), 3);
    assert!(first.iter().all(|c| c.flags == recalc::ALL));
    assert!(bmain.take_changes().is_empty());

    bmain.object_mut(cube).unwrap().location = Vec3::X;
    assert_eq!(
        bmain.take_changes(),
        vec![Change { id_type: IdType::Object, handle: cube.0, flags: recalc::TRANSFORM }]
    );

    bmain.mesh_mut(mesh).unwrap().positions_mut()[0] = Vec3::ZERO;
    assert_eq!(
        bmain.take_changes(),
        vec![Change { id_type: IdType::Mesh, handle: mesh.0, flags: recalc::GEOMETRY }]
    );

    bmain.object_remove(cube).unwrap();
    assert_eq!(
        bmain.take_changes(),
        vec![Change { id_type: IdType::Scene, handle: scene.0, flags: recalc::RELATIONS }]
    );
}

#[test]
fn session_uids_are_unique() {
    let mut bmain = Main::new();
    let scene = bmain.scene_add("Scene");
    let ob = bmain.object_add("Cube", ObjectData::Empty).unwrap();
    let uid_scene = bmain.scene(scene).unwrap().id.session_uid;
    let uid_ob = bmain.object(ob).unwrap().id.session_uid;
    assert_ne!(uid_scene, uid_ob);
    assert!(uid_scene > 0 && uid_ob > 0);
}

#[test]
fn rename_keeps_names_unique() {
    let mut bmain = Main::new();
    let a = bmain.object_add("Cube", ObjectData::Empty).unwrap();
    let b = bmain.object_add("Sphere", ObjectData::Empty).unwrap();
    assert_eq!(bmain.id_rename(IdType::Object, b.0, "Cube").unwrap(), "Cube.001");
    assert_eq!(bmain.id_rename(IdType::Object, a.0, "Floor").unwrap(), "Floor");
    // The old name is free again.
    assert_eq!(bmain.id_rename(IdType::Object, b.0, "Cube").unwrap(), "Cube");
    assert_eq!(bmain.id_rename(IdType::Object, b.0, "Cube").unwrap(), "Cube", "same name is a no-op");
    assert!(bmain.id_rename(IdType::Object, 99, "x").is_err());
}
