//! Port of `struct Object` transforms (`makesdna/DNA_object_types.h`,
//! `blenkernel/intern/object.cc`: `BKE_object_rot_to_mat3`, `BKE_object_to_mat3`,
//! `BKE_object_to_mat4`).

use glam::{Mat3, Mat4, Quat, Vec3};

use crate::id::IdHeader;
use crate::mesh::MeshId;

/// Handle of an object in a [`crate::Main`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectId(pub u32);

/// What the object instances (`Object::type` + `Object::data`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectData {
    Empty,
    Mesh(MeshId),
}

/// `Object::rotmode`. The values match Blender's `ROT_MODE_*`.
/// Axis-angle (`ROT_MODE_AXISANGLE = -1`) is not supported yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum RotationMode {
    Quaternion = 0,
    EulerXYZ = 1,
    EulerXZY = 2,
    EulerYXZ = 3,
    EulerYZX = 4,
    EulerZXY = 5,
    EulerZYX = 6,
}

impl RotationMode {
    pub fn from_i32(value: i32) -> Option<RotationMode> {
        Some(match value {
            0 => RotationMode::Quaternion,
            1 => RotationMode::EulerXYZ,
            2 => RotationMode::EulerXZY,
            3 => RotationMode::EulerYXZ,
            4 => RotationMode::EulerYZX,
            5 => RotationMode::EulerZXY,
            6 => RotationMode::EulerZYX,
            _ => return None,
        })
    }

    /// Axes in the order they are applied (0 = X, 1 = Y, 2 = Z); `None` for quaternions.
    fn euler_order(self) -> Option<[usize; 3]> {
        match self {
            RotationMode::Quaternion => None,
            RotationMode::EulerXYZ => Some([0, 1, 2]),
            RotationMode::EulerXZY => Some([0, 2, 1]),
            RotationMode::EulerYXZ => Some([1, 0, 2]),
            RotationMode::EulerYZX => Some([1, 2, 0]),
            RotationMode::EulerZXY => Some([2, 0, 1]),
            RotationMode::EulerZYX => Some([2, 1, 0]),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub id: IdHeader,
    pub(crate) data: ObjectData,
    pub(crate) parent: Option<ObjectId>,
    /// `Object::loc` / `dloc`.
    pub location: Vec3,
    pub delta_location: Vec3,
    /// `Object::rot` / `drot`, radians.
    pub rotation_euler: Vec3,
    pub delta_rotation_euler: Vec3,
    /// `Object::quat` / `dquat`.
    pub rotation_quaternion: Quat,
    pub delta_rotation_quaternion: Quat,
    /// `Object::scale` / `dscale`.
    pub scale: Vec3,
    pub delta_scale: Vec3,
    pub rotation_mode: RotationMode,
}

impl Object {
    /// Defaults from `DNA_object_defaults.h`: identity transform, Euler XYZ.
    pub fn new(data: ObjectData) -> Object {
        Object {
            id: IdHeader::default(),
            data,
            parent: None,
            location: Vec3::ZERO,
            delta_location: Vec3::ZERO,
            rotation_euler: Vec3::ZERO,
            delta_rotation_euler: Vec3::ZERO,
            rotation_quaternion: Quat::IDENTITY,
            delta_rotation_quaternion: Quat::IDENTITY,
            scale: Vec3::ONE,
            delta_scale: Vec3::ONE,
            rotation_mode: RotationMode::EulerXYZ,
        }
    }

    pub fn data(&self) -> ObjectData {
        self.data
    }

    pub fn parent(&self) -> Option<ObjectId> {
        self.parent
    }

    pub fn mesh(&self) -> Option<MeshId> {
        match self.data {
            ObjectData::Mesh(mesh) => Some(mesh),
            ObjectData::Empty => None,
        }
    }

    /// `BKE_object_rot_to_mat3(ob, mat, use_drot = true)`: delta rotation × rotation.
    pub fn rotation_matrix(&self) -> Mat3 {
        match self.rotation_mode.euler_order() {
            Some(order) => {
                euler_to_mat3(self.delta_rotation_euler, order) * euler_to_mat3(self.rotation_euler, order)
            }
            None => {
                Mat3::from_quat(normalized_or_identity(self.delta_rotation_quaternion))
                    * Mat3::from_quat(normalized_or_identity(self.rotation_quaternion))
            }
        }
    }

    /// `BKE_object_to_mat4`: rotation × scale, then translation. Ignores the parent.
    pub fn local_matrix(&self) -> Mat4 {
        let rot_scale = self.rotation_matrix() * Mat3::from_diagonal(self.scale * self.delta_scale);
        let mut matrix = Mat4::from_mat3(rot_scale);
        matrix.w_axis = (self.location + self.delta_location).extend(1.0);
        matrix
    }
}

/// `eulO_to_mat3`: apply the axis rotations in `order`, so XYZ = Rz · Ry · Rx.
fn euler_to_mat3(euler: Vec3, order: [usize; 3]) -> Mat3 {
    order.iter().fold(Mat3::IDENTITY, |matrix, &axis| {
        let angle = euler[axis];
        let rotation = match axis {
            0 => Mat3::from_rotation_x(angle),
            1 => Mat3::from_rotation_y(angle),
            _ => Mat3::from_rotation_z(angle),
        };
        rotation * matrix
    })
}

/// Blender normalizes quaternions before use; a zero quaternion becomes identity here.
fn normalized_or_identity(q: Quat) -> Quat {
    if q.length_squared() > 0.0 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}
