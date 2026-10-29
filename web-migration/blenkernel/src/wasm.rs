//! JavaScript API (`wasm-bindgen`). This is the contract with the depsgraph and render tickets;
//! see PLAN.md §5.
//!
//! - Every data-block is a `u32` handle. [`NONE`] (`0xFFFFFFFF`) means "no data-block".
//! - Buffer getters return *copies* as typed arrays (`Float32Array`, `Uint32Array`), so they stay
//!   valid when WASM memory grows.
//! - Matrices are 16 floats, column-major, translation in elements 12..14 (WebGPU layout).
//! - Invalid handles throw a JavaScript `Error`.

use glam::{Quat, Vec3};
use wasm_bindgen::prelude::*;

use crate::collection::CollectionId;
use crate::error::BkeError;
use crate::main_db::{CollectionRef, Main};
use crate::mesh::{Mesh, MeshId};
use crate::object::{ObjectData, ObjectId, RotationMode};
use crate::primitives;
use crate::scene::SceneId;

/// Handle value meaning "none" (no parent, no mesh, ...).
pub const NONE: u32 = u32::MAX;

fn js_err(error: BkeError) -> JsError {
    JsError::new(&error.to_string())
}

fn flatten3(values: &[Vec3]) -> Vec<f32> {
    values.iter().flat_map(|v| v.to_array()).collect()
}

fn vec3s_from_flat(values: &[f32]) -> Result<Vec<Vec3>, BkeError> {
    if values.len() % 3 != 0 {
        return Err(BkeError::LengthMismatch {
            what: "flat float3 array",
            expected: values.len() / 3 * 3,
            got: values.len(),
        });
    }
    Ok(values.chunks_exact(3).map(Vec3::from_slice).collect())
}

/// The whole Blender database, exposed to JavaScript as `BlenderMain`.
#[wasm_bindgen]
pub struct BlenderMain {
    inner: Main,
}

impl Default for BlenderMain {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl BlenderMain {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BlenderMain {
        BlenderMain { inner: Main::new() }
    }

    // ------------------------------------------------------------ Scenes & collections

    pub fn scene_add(&mut self, name: &str) -> u32 {
        self.inner.scene_add(name).0
    }

    pub fn collection_add(&mut self, name: &str) -> u32 {
        self.inner.collection_add(name).0
    }

    /// Link an object into the scene's master collection.
    pub fn scene_link_object(&mut self, scene: u32, object: u32) -> Result<bool, JsError> {
        self.inner
            .collection_link_object(CollectionRef::Scene(SceneId(scene)), ObjectId(object))
            .map_err(js_err)
    }

    pub fn scene_unlink_object(&mut self, scene: u32, object: u32) -> Result<bool, JsError> {
        self.inner
            .collection_unlink_object(CollectionRef::Scene(SceneId(scene)), ObjectId(object))
            .map_err(js_err)
    }

    /// Link a collection under the scene's master collection.
    pub fn scene_link_collection(&mut self, scene: u32, collection: u32) -> Result<bool, JsError> {
        self.inner
            .collection_link_child(CollectionRef::Scene(SceneId(scene)), CollectionId(collection))
            .map_err(js_err)
    }

    pub fn collection_link_object(&mut self, collection: u32, object: u32) -> Result<bool, JsError> {
        self.inner
            .collection_link_object(CollectionRef::Collection(CollectionId(collection)), ObjectId(object))
            .map_err(js_err)
    }

    pub fn collection_link_collection(&mut self, parent: u32, child: u32) -> Result<bool, JsError> {
        self.inner
            .collection_link_child(CollectionRef::Collection(CollectionId(parent)), CollectionId(child))
            .map_err(js_err)
    }

    /// All objects in the scene (nested collections included), each once.
    pub fn scene_objects(&self, scene: u32) -> Result<Vec<u32>, JsError> {
        let objects = self.inner.scene_objects(SceneId(scene)).map_err(js_err)?;
        Ok(objects.into_iter().map(|o| o.0).collect())
    }

    /// Pass `NONE` to clear.
    pub fn scene_set_camera(&mut self, scene: u32, object: u32) -> Result<(), JsError> {
        let camera = (object != NONE).then_some(ObjectId(object));
        self.inner.scene_set_camera(SceneId(scene), camera).map_err(js_err)
    }

    /// Camera object handle, or `NONE`.
    pub fn scene_camera(&self, scene: u32) -> Result<u32, JsError> {
        let found = self.inner.scene(SceneId(scene)).map_err(js_err)?;
        Ok(found.camera.map_or(NONE, |c| c.0))
    }

    // ------------------------------------------------------------ Meshes

    /// Box of the given edge length, 8 vertices / 6 quads. Blender's default cube is size 2.
    pub fn mesh_add_cube(&mut self, size: f32) -> u32 {
        self.inner.mesh_add("Cube", primitives::cube(Vec3::splat(size))).0
    }

    pub fn mesh_add_grid(&mut self, verts_x: u32, verts_y: u32, size_x: f32, size_y: f32) -> u32 {
        self.inner.mesh_add("Grid", primitives::grid(verts_x, verts_y, size_x, size_y)).0
    }

    /// Blender's defaults: radius 1, 32 segments, 16 rings.
    pub fn mesh_add_uv_sphere(&mut self, radius: f32, segments: u32, rings: u32) -> u32 {
        self.inner.mesh_add("Sphere", primitives::uv_sphere(radius, segments, rings)).0
    }

    /// Mesh from raw data: `positions` = xyz per vertex, `face_offsets` = faces + 1 entries
    /// starting at 0, `corner_verts` = vertex index per face corner.
    pub fn mesh_add_from_data(
        &mut self,
        name: &str,
        positions: Vec<f32>,
        face_offsets: Vec<i32>,
        corner_verts: Vec<i32>,
    ) -> Result<u32, JsError> {
        let positions = vec3s_from_flat(&positions).map_err(js_err)?;
        let mesh = Mesh::from_faces(positions, face_offsets, corner_verts).map_err(js_err)?;
        Ok(self.inner.mesh_add(name, mesh).0)
    }

    /// Fails while an object still uses the mesh.
    pub fn mesh_remove(&mut self, mesh: u32) -> Result<(), JsError> {
        self.inner.mesh_remove(MeshId(mesh)).map_err(js_err)
    }

    pub fn mesh_name(&self, mesh: u32) -> Result<String, JsError> {
        Ok(self.mesh(mesh)?.id.name.clone())
    }

    /// `[verts, edges, faces, corners]`.
    pub fn mesh_counts(&self, mesh: u32) -> Result<Vec<u32>, JsError> {
        let mesh = self.mesh(mesh)?;
        Ok(vec![
            mesh.verts_num() as u32,
            mesh.edges_num() as u32,
            mesh.faces_num() as u32,
            mesh.corners_num() as u32,
        ])
    }

    /// `Float32Array(verts * 3)`.
    pub fn mesh_positions(&self, mesh: u32) -> Result<Vec<f32>, JsError> {
        Ok(flatten3(self.mesh(mesh)?.positions()))
    }

    /// Replace all positions (same vertex count); tags a geometry update.
    pub fn mesh_set_positions(&mut self, mesh: u32, positions: Vec<f32>) -> Result<(), JsError> {
        let positions = vec3s_from_flat(&positions).map_err(js_err)?;
        self.inner.mesh_mut(MeshId(mesh)).map_err(js_err)?.set_positions(positions).map_err(js_err)
    }

    /// `Float32Array(verts * 3)`, unit length.
    pub fn mesh_vert_normals(&self, mesh: u32) -> Result<Vec<f32>, JsError> {
        Ok(flatten3(self.mesh(mesh)?.vert_normals()))
    }

    /// `Float32Array(faces * 3)`, unit length.
    pub fn mesh_face_normals(&self, mesh: u32) -> Result<Vec<f32>, JsError> {
        Ok(flatten3(self.mesh(mesh)?.face_normals()))
    }

    /// `Uint32Array(tris * 3)`: vertex indices, counter-clockwise seen from outside.
    pub fn mesh_triangles(&self, mesh: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.mesh(mesh)?.tri_vert_indices())
    }

    /// `Uint32Array(edges * 2)`: vertex index pairs, for wireframes.
    pub fn mesh_edges(&self, mesh: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.mesh(mesh)?.edges().iter().flat_map(|e| e.map(|v| v as u32)).collect())
    }

    /// `Uint32Array(faces + 1)`: corner offsets per face.
    pub fn mesh_face_offsets(&self, mesh: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.mesh(mesh)?.face_offsets().iter().map(|&o| o as u32).collect())
    }

    /// `Uint32Array(corners)`: vertex index per face corner.
    pub fn mesh_corner_verts(&self, mesh: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.mesh(mesh)?.corner_verts().iter().map(|&v| v as u32).collect())
    }

    /// `Float32Array(6)`: min xyz, max xyz. Empty array for a mesh without vertices.
    pub fn mesh_bounds(&self, mesh: u32) -> Result<Vec<f32>, JsError> {
        Ok(match self.mesh(mesh)?.bounds() {
            Some(b) => [b.min.to_array(), b.max.to_array()].concat(),
            None => Vec::new(),
        })
    }

    // ------------------------------------------------------------ Objects

    pub fn object_add_mesh(&mut self, name: &str, mesh: u32) -> Result<u32, JsError> {
        self.inner
            .object_add(name, ObjectData::Mesh(MeshId(mesh)))
            .map(|o| o.0)
            .map_err(js_err)
    }

    pub fn object_add_empty(&mut self, name: &str) -> u32 {
        self.inner.object_add(name, ObjectData::Empty).expect("empty objects always succeed").0
    }

    pub fn object_remove(&mut self, object: u32) -> Result<(), JsError> {
        self.inner.object_remove(ObjectId(object)).map_err(js_err)
    }

    pub fn object_name(&self, object: u32) -> Result<String, JsError> {
        Ok(self.inner.object(ObjectId(object)).map_err(js_err)?.id.name.clone())
    }

    /// Mesh handle, or `NONE` for an empty.
    pub fn object_mesh(&self, object: u32) -> Result<u32, JsError> {
        let found = self.inner.object(ObjectId(object)).map_err(js_err)?;
        Ok(found.mesh().map_or(NONE, |m| m.0))
    }

    /// Pass `NONE` to make it an empty.
    pub fn object_set_mesh(&mut self, object: u32, mesh: u32) -> Result<(), JsError> {
        let data = if mesh == NONE { ObjectData::Empty } else { ObjectData::Mesh(MeshId(mesh)) };
        self.inner.object_set_data(ObjectId(object), data).map_err(js_err)
    }

    /// Parent handle, or `NONE`.
    pub fn object_parent(&self, object: u32) -> Result<u32, JsError> {
        let found = self.inner.object(ObjectId(object)).map_err(js_err)?;
        Ok(found.parent().map_or(NONE, |p| p.0))
    }

    /// Pass `NONE` to clear. Throws if it would create a cycle.
    pub fn object_set_parent(&mut self, child: u32, parent: u32) -> Result<(), JsError> {
        let parent = if parent == NONE { None } else { Some(ObjectId(parent)) };
        self.inner.object_set_parent(ObjectId(child), parent).map_err(js_err)
    }

    pub fn object_set_location(&mut self, object: u32, x: f32, y: f32, z: f32) -> Result<(), JsError> {
        self.object_mut(object)?.location = Vec3::new(x, y, z);
        Ok(())
    }

    /// Radians. Used when the rotation mode is an Euler mode (the default).
    pub fn object_set_rotation_euler(&mut self, object: u32, x: f32, y: f32, z: f32) -> Result<(), JsError> {
        self.object_mut(object)?.rotation_euler = Vec3::new(x, y, z);
        Ok(())
    }

    /// Blender order: w first. Used when the rotation mode is quaternion (0).
    pub fn object_set_rotation_quaternion(&mut self, object: u32, w: f32, x: f32, y: f32, z: f32) -> Result<(), JsError> {
        self.object_mut(object)?.rotation_quaternion = Quat::from_xyzw(x, y, z, w);
        Ok(())
    }

    /// Blender's `ROT_MODE_*`: 0 = quaternion, 1 = XYZ Euler (default), 2 = XZY, 3 = YXZ,
    /// 4 = YZX, 5 = ZXY, 6 = ZYX.
    pub fn object_set_rotation_mode(&mut self, object: u32, mode: i32) -> Result<(), JsError> {
        let mode = RotationMode::from_i32(mode)
            .ok_or_else(|| JsError::new(&format!("unsupported rotation mode {mode}")))?;
        self.object_mut(object)?.rotation_mode = mode;
        Ok(())
    }

    pub fn object_set_scale(&mut self, object: u32, x: f32, y: f32, z: f32) -> Result<(), JsError> {
        self.object_mut(object)?.scale = Vec3::new(x, y, z);
        Ok(())
    }

    /// `Float32Array(3)`.
    pub fn object_location(&self, object: u32) -> Result<Vec<f32>, JsError> {
        Ok(self.inner.object(ObjectId(object)).map_err(js_err)?.location.to_array().to_vec())
    }

    /// `Float32Array(16)`, column-major, relative to the parent.
    pub fn object_local_matrix(&self, object: u32) -> Result<Vec<f32>, JsError> {
        let matrix = self.inner.object_local_matrix(ObjectId(object)).map_err(js_err)?;
        Ok(matrix.to_cols_array().to_vec())
    }

    /// `Float32Array(16)`, column-major, world space (parent chain applied).
    pub fn object_world_matrix(&self, object: u32) -> Result<Vec<f32>, JsError> {
        let matrix = self.inner.object_world_matrix(ObjectId(object)).map_err(js_err)?;
        Ok(matrix.to_cols_array().to_vec())
    }

    // ------------------------------------------------------------ Updates (depsgraph)

    /// Pending updates since the last call, then cleared:
    /// `Uint32Array [type, handle, flags, type, handle, flags, ...]`.
    /// Types: 0 scene, 1 object, 2 mesh, 3 collection.
    /// Flags: 1 transform, 2 geometry, 0x80000000 relations.
    pub fn take_changes(&mut self) -> Vec<u32> {
        self.inner
            .take_changes()
            .into_iter()
            .flat_map(|c| [c.id_type as u32, c.handle, c.flags])
            .collect()
    }
}

impl BlenderMain {
    /// The Rust-side database, for code linking this crate directly.
    pub fn main(&self) -> &Main {
        &self.inner
    }

    pub fn main_mut(&mut self) -> &mut Main {
        &mut self.inner
    }

    fn mesh(&self, mesh: u32) -> Result<&Mesh, JsError> {
        self.inner.mesh(MeshId(mesh)).map_err(js_err)
    }

    fn object_mut(&mut self, object: u32) -> Result<&mut crate::Object, JsError> {
        self.inner.object_mut(ObjectId(object)).map_err(js_err)
    }
}
