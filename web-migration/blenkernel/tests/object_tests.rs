//! Object transform checks against `BKE_object_to_mat4` (PLAN.md §7).

use std::f32::consts::FRAC_PI_2;

use blenkernel::{BkeError, Main, Object, ObjectData, RotationMode};
use glam::{Mat4, Quat, Vec3};

const EPS: f32 = 1e-5;

#[test]
fn default_object_is_identity() {
    let object = Object::new(ObjectData::Empty);
    assert!(object.local_matrix().abs_diff_eq(Mat4::IDENTITY, EPS));
}

#[test]
fn location_goes_into_matrix_elements_12_to_14() {
    let mut object = Object::new(ObjectData::Empty);
    object.location = Vec3::new(1.0, 2.0, 3.0);
    let m = object.local_matrix().to_cols_array();
    assert_eq!(&m[12..16], &[1.0, 2.0, 3.0, 1.0]);
}

#[test]
fn rotation_z_90_maps_x_to_y() {
    let mut object = Object::new(ObjectData::Empty);
    object.rotation_euler = Vec3::new(0.0, 0.0, FRAC_PI_2);
    let p = object.local_matrix().transform_point3(Vec3::X);
    assert!(p.abs_diff_eq(Vec3::Y, EPS), "{p}");
}

#[test]
fn euler_xyz_applies_x_first() {
    // XYZ order: rotate about X, then Y, then Z (eulO_to_mat3 with EULER_ORDER_XYZ).
    let mut object = Object::new(ObjectData::Empty);
    object.rotation_euler = Vec3::new(FRAC_PI_2, 0.0, FRAC_PI_2);
    // +Y --X 90--> +Z --Z 90--> +Z.
    let p = object.local_matrix().transform_point3(Vec3::Y);
    assert!(p.abs_diff_eq(Vec3::Z, EPS), "{p}");

    object.rotation_mode = RotationMode::EulerZYX;
    // +Y --Z 90--> -X --X 90--> -X.
    let p = object.local_matrix().transform_point3(Vec3::Y);
    assert!(p.abs_diff_eq(Vec3::NEG_X, EPS), "{p}");
}

#[test]
fn quaternion_mode_and_scale() {
    let mut object = Object::new(ObjectData::Empty);
    object.rotation_mode = RotationMode::Quaternion;
    // Not normalized on purpose: Blender normalizes before use.
    object.rotation_quaternion = Quat::from_rotation_z(FRAC_PI_2) * 3.0;
    object.scale = Vec3::new(2.0, 1.0, 1.0);
    // Scale first, then rotate: X * 2 -> Y * 2.
    let p = object.local_matrix().transform_point3(Vec3::X);
    assert!(p.abs_diff_eq(Vec3::new(0.0, 2.0, 0.0), EPS), "{p}");
}

#[test]
fn delta_transforms_combine() {
    let mut object = Object::new(ObjectData::Empty);
    object.location = Vec3::X;
    object.delta_location = Vec3::Y;
    object.scale = Vec3::splat(2.0);
    object.delta_scale = Vec3::splat(3.0);
    let p = object.local_matrix().transform_point3(Vec3::X);
    assert!(p.abs_diff_eq(Vec3::new(7.0, 1.0, 0.0), EPS), "{p}");
}

#[test]
fn world_matrix_applies_parent_chain() {
    let mut bmain = Main::new();
    let parent = bmain.object_add("Parent", ObjectData::Empty).unwrap();
    let child = bmain.object_add("Child", ObjectData::Empty).unwrap();
    bmain.object_set_parent(child, Some(parent)).unwrap();

    bmain.object_mut(parent).unwrap().location = Vec3::new(10.0, 0.0, 0.0);
    bmain.object_mut(parent).unwrap().rotation_euler = Vec3::new(0.0, 0.0, FRAC_PI_2);
    bmain.object_mut(child).unwrap().location = Vec3::new(1.0, 0.0, 0.0);

    // Child's origin: (1,0,0) rotated 90° about Z = (0,1,0), then + (10,0,0).
    let world = bmain.object_world_matrix(child).unwrap();
    assert!(world.transform_point3(Vec3::ZERO).abs_diff_eq(Vec3::new(10.0, 1.0, 0.0), EPS));
    // Local matrix ignores the parent.
    let local = bmain.object_local_matrix(child).unwrap();
    assert!(local.transform_point3(Vec3::ZERO).abs_diff_eq(Vec3::X, EPS));
}

#[test]
fn parent_cycles_are_rejected() {
    let mut bmain = Main::new();
    let a = bmain.object_add("A", ObjectData::Empty).unwrap();
    let b = bmain.object_add("B", ObjectData::Empty).unwrap();
    let c = bmain.object_add("C", ObjectData::Empty).unwrap();
    bmain.object_set_parent(b, Some(a)).unwrap();
    bmain.object_set_parent(c, Some(b)).unwrap();
    assert_eq!(bmain.object_set_parent(a, Some(c)), Err(BkeError::ParentCycle));
    assert_eq!(bmain.object_set_parent(a, Some(a)), Err(BkeError::ParentCycle));
    bmain.object_set_parent(c, None).unwrap();
    assert_eq!(bmain.object(c).unwrap().parent(), None);
}
