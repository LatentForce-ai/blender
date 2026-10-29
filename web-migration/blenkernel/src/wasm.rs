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
use crate::id::{recalc, IdType};
use crate::main_db::{CollectionRef, Main};
use crate::material::{default_surface_tree, MaterialId};
use crate::mesh::{Mesh, MeshId};
use crate::node::{NodeId, NodeTree, SocketInOut, SocketValue};
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

    /// `Float32Array(3)`, radians.
    pub fn object_rotation_euler(&self, object: u32) -> Result<Vec<f32>, JsError> {
        Ok(self.inner.object(ObjectId(object)).map_err(js_err)?.rotation_euler.to_array().to_vec())
    }

    /// `Float32Array(4)`, Blender order: w, x, y, z.
    pub fn object_rotation_quaternion(&self, object: u32) -> Result<Vec<f32>, JsError> {
        let q = self.inner.object(ObjectId(object)).map_err(js_err)?.rotation_quaternion;
        Ok(vec![q.w, q.x, q.y, q.z])
    }

    /// Blender's `ROT_MODE_*` value, see `object_set_rotation_mode`.
    pub fn object_rotation_mode(&self, object: u32) -> Result<i32, JsError> {
        Ok(self.inner.object(ObjectId(object)).map_err(js_err)?.rotation_mode as i32)
    }

    /// `Float32Array(3)`.
    pub fn object_scale(&self, object: u32) -> Result<Vec<f32>, JsError> {
        Ok(self.inner.object(ObjectId(object)).map_err(js_err)?.scale.to_array().to_vec())
    }

    /// Rename; returns the unique name it got ("Cube.001" if "Cube" is taken).
    pub fn object_rename(&mut self, object: u32, name: &str) -> Result<String, JsError> {
        self.inner.id_rename(IdType::Object, object, name).map_err(js_err)
    }

    /// Rename; returns the unique name it got.
    pub fn material_rename(&mut self, material: u32, name: &str) -> Result<String, JsError> {
        self.inner.id_rename(IdType::Material, material, name).map_err(js_err)
    }

    /// Every material handle, in creation order.
    pub fn materials(&self) -> Vec<u32> {
        self.inner.materials().map(|(id, _)| id.0).collect()
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

    // ------------------------------------------------------------ Materials (shading / render)

    /// New material. `use_nodes` gives it Blender's default tree:
    /// Principled BSDF → Material Output.
    pub fn material_add(&mut self, name: &str, use_nodes: bool) -> u32 {
        self.inner.material_add(name, use_nodes).0
    }

    /// Fails while a mesh slot still uses it.
    pub fn material_remove(&mut self, material: u32) -> Result<(), JsError> {
        self.inner.material_remove(MaterialId(material)).map_err(js_err)
    }

    pub fn material_name(&self, material: u32) -> Result<String, JsError> {
        Ok(self.inner.material(MaterialId(material)).map_err(js_err)?.id.name.clone())
    }

    /// Viewport display values, what Solid mode draws:
    /// `Float32Array [r, g, b, a, metallic, roughness]`.
    pub fn material_viewport(&self, material: u32) -> Result<Vec<f32>, JsError> {
        let found = self.inner.material(MaterialId(material)).map_err(js_err)?;
        let mut values = found.base_color.to_vec();
        values.extend([found.metallic, found.roughness]);
        Ok(values)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn material_set_viewport(
        &mut self,
        material: u32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
        metallic: f32,
        roughness: f32,
    ) -> Result<(), JsError> {
        let found = self.inner.material_mut(MaterialId(material)).map_err(js_err)?;
        found.base_color = [r, g, b, a];
        found.metallic = metallic;
        found.roughness = roughness;
        Ok(())
    }

    pub fn material_has_node_tree(&self, material: u32) -> Result<bool, JsError> {
        Ok(self.inner.material(MaterialId(material)).map_err(js_err)?.node_tree.is_some())
    }

    /// Give a material without nodes the default tree, using its viewport color.
    pub fn material_use_nodes(&mut self, material: u32) -> Result<(), JsError> {
        let found = self.inner.material_mut(MaterialId(material)).map_err(js_err)?;
        if found.node_tree.is_none() {
            found.node_tree = Some(default_surface_tree(found.base_color));
        }
        Ok(())
    }

    /// The node tree as JSON: `{"output_node": id|null, "nodes": [{id, name, idname, location,
    /// inputs: [{identifier, name, type, default_value}], outputs, properties}], "links":
    /// [{from_node, from_socket, to_node, to_socket}]}`.
    pub fn material_node_tree_json(&self, material: u32) -> Result<String, JsError> {
        Ok(self.tree(material)?.to_json())
    }

    /// Node handles in creation order.
    pub fn material_nodes(&self, material: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.tree(material)?.nodes().map(|(id, _)| id.0).collect())
    }

    /// Node handles ordered so inputs come before the nodes using them (evaluation order).
    pub fn material_node_order(&self, material: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.tree(material)?.topological_order().into_iter().map(|n| n.0).collect())
    }

    /// The active Material Output node, or `NONE`.
    pub fn material_output_node(&self, material: u32) -> Result<u32, JsError> {
        Ok(self.tree(material)?.output_node().map_or(NONE, |n| n.0))
    }

    /// Add a node by type, e.g. "ShaderNodeBsdfPrincipled", "ShaderNodeMix", "ShaderNodeRGB".
    pub fn material_node_add(&mut self, material: u32, idname: &str) -> Result<u32, JsError> {
        Ok(self.tree_mut(material)?.add_node(idname).map_err(js_err)?.0)
    }

    pub fn material_node_remove(&mut self, material: u32, node: u32) -> Result<(), JsError> {
        self.tree_mut(material)?.remove_node(NodeId(node)).map_err(js_err)
    }

    pub fn material_node_idname(&self, material: u32, node: u32) -> Result<String, JsError> {
        Ok(self.tree(material)?.node(NodeId(node)).map_err(js_err)?.idname.clone())
    }

    pub fn material_node_name(&self, material: u32, node: u32) -> Result<String, JsError> {
        Ok(self.tree(material)?.node(NodeId(node)).map_err(js_err)?.name.clone())
    }

    pub fn material_node_set_location(&mut self, material: u32, node: u32, x: f32, y: f32) -> Result<(), JsError> {
        self.tree_mut(material)?.set_location(NodeId(node), x, y).map_err(js_err)
    }

    /// Input socket default value as floats (1 for float/bool, 3 for vector, 4 for color).
    pub fn material_node_input(&self, material: u32, node: u32, socket: &str) -> Result<Vec<f32>, JsError> {
        let found = self.tree(material)?.node(NodeId(node)).map_err(js_err)?;
        let socket = found
            .input(socket)
            .ok_or_else(|| JsError::new(&format!("{} has no input \"{socket}\"", found.name)))?;
        Ok(socket.default_value.to_floats())
    }

    /// Set an input socket's default value (same float layout as `material_node_input`).
    pub fn material_node_set_input(&mut self, material: u32, node: u32, socket: &str, values: Vec<f32>) -> Result<(), JsError> {
        self.set_socket(material, node, SocketInOut::Input, socket, &values)
    }

    /// Set an output socket's stored value, used by RGB and Value nodes.
    pub fn material_node_set_output(&mut self, material: u32, node: u32, socket: &str, values: Vec<f32>) -> Result<(), JsError> {
        self.set_socket(material, node, SocketInOut::Output, socket, &values)
    }

    /// Set a node setting, e.g. Math "operation" = "MULTIPLY", Mix "blend_type" = "MULTIPLY",
    /// Image Texture "image" = "wood.png".
    pub fn material_node_set_property(&mut self, material: u32, node: u32, key: &str, value: &str) -> Result<(), JsError> {
        self.tree_mut(material)?.set_property(NodeId(node), key, value).map_err(js_err)
    }

    /// Link an output socket to an input socket; replaces an existing link into that input.
    pub fn material_link_add(
        &mut self,
        material: u32,
        from_node: u32,
        from_socket: &str,
        to_node: u32,
        to_socket: &str,
    ) -> Result<(), JsError> {
        self.tree_mut(material)?
            .add_link(NodeId(from_node), from_socket, NodeId(to_node), to_socket)
            .map_err(js_err)
    }

    /// Remove the link into an input; false if there was none.
    pub fn material_link_remove(&mut self, material: u32, to_node: u32, to_socket: &str) -> Result<bool, JsError> {
        Ok(self.tree_mut(material)?.remove_link(NodeId(to_node), to_socket))
    }

    /// Replace the mesh's material slots; `NONE` is an empty slot.
    pub fn mesh_set_materials(&mut self, mesh: u32, materials: Vec<u32>) -> Result<(), JsError> {
        let slots = materials
            .into_iter()
            .map(|m| (m != NONE).then_some(MaterialId(m)))
            .collect();
        self.inner.mesh_set_materials(MeshId(mesh), slots).map_err(js_err)
    }

    /// `Uint32Array` of material handles per slot, `NONE` for empty slots.
    pub fn mesh_materials(&self, mesh: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.mesh(mesh)?.materials().iter().map(|m| m.map_or(NONE, |m| m.0)).collect())
    }

    /// Material slot per face (`Int32Array`, one per face).
    pub fn mesh_set_material_indices(&mut self, mesh: u32, indices: Vec<i32>) -> Result<(), JsError> {
        self.inner
            .mesh_mut(MeshId(mesh))
            .map_err(js_err)?
            .set_material_indices(indices)
            .map_err(js_err)?;
        self.inner.tag(IdType::Mesh, mesh, recalc::SHADING).map_err(js_err)
    }

    pub fn mesh_material_indices(&self, mesh: u32) -> Result<Vec<i32>, JsError> {
        Ok(self.mesh(mesh)?.material_indices())
    }

    /// Material slot per triangle of `mesh_triangles`, clamped to existing slots.
    pub fn mesh_tri_material_indices(&self, mesh: u32) -> Result<Vec<u32>, JsError> {
        Ok(self.mesh(mesh)?.tri_material_indices())
    }

    /// Material used by an object's slot (clamped), or `NONE`.
    pub fn object_material(&self, object: u32, slot: u32) -> Result<u32, JsError> {
        let material = self.inner.object_material(ObjectId(object), slot as usize).map_err(js_err)?;
        Ok(material.map_or(NONE, |m| m.0))
    }

    // ------------------------------------------------------------ Updates (depsgraph)

    /// Pending updates since the last call, then cleared:
    /// `Uint32Array [type, handle, flags, type, handle, flags, ...]`.
    /// Types: 0 scene, 1 object, 2 mesh, 3 collection, 4 material.
    /// Flags: 1 transform, 2 geometry, 0x80 shading, 0x80000000 relations.
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

    fn tree(&self, material: u32) -> Result<&NodeTree, JsError> {
        let found = self.inner.material(MaterialId(material)).map_err(js_err)?;
        found.node_tree.as_ref().ok_or_else(|| js_err(BkeError::NoNodeTree(material)))
    }

    /// Node tree for editing; tags the material for a shading update.
    fn tree_mut(&mut self, material: u32) -> Result<&mut NodeTree, JsError> {
        let found = self.inner.material_mut(MaterialId(material)).map_err(js_err)?;
        found.node_tree.as_mut().ok_or_else(|| js_err(BkeError::NoNodeTree(material)))
    }

    fn set_socket(&mut self, material: u32, node: u32, in_out: SocketInOut, socket: &str, values: &[f32]) -> Result<(), JsError> {
        let tree = self.tree_mut(material)?;
        let found = tree.node(NodeId(node)).map_err(js_err)?;
        let sockets = match in_out {
            SocketInOut::Input => &found.inputs,
            SocketInOut::Output => &found.outputs,
        };
        let socket_type = sockets
            .iter()
            .find(|s| s.identifier == socket)
            .ok_or_else(|| JsError::new(&format!("{} has no socket \"{socket}\"", found.name)))?
            .socket_type;
        let value = SocketValue::from_floats(socket_type, values).ok_or_else(|| {
            JsError::new(&format!("\"{socket}\" is {socket_type:?}; got {} value(s)", values.len()))
        })?;
        tree.set_socket_value(NodeId(node), in_out, socket, value).map_err(js_err)
    }

    fn object_mut(&mut self, object: u32) -> Result<&mut crate::Object, JsError> {
        self.inner.object_mut(ObjectId(object)).map_err(js_err)
    }
}
