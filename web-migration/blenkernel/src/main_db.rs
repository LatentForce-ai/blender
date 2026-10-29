//! Port of `struct Main`, the database of all data-blocks (`blenkernel/BKE_main.hh`,
//! `intern/main.cc`, `intern/main_namemap.cc`, `intern/lib_id.cc`).
//!
//! C++ keeps one `ListBase` per ID type and links data-blocks with pointers. Here each type lives
//! in an arena and is referenced by a `u32` handle. Handles are never reused, so a handle to a
//! removed data-block is reported as invalid instead of pointing at something else.

use std::collections::{HashMap, HashSet};

use glam::Mat4;

use crate::collection::{Collection, CollectionId};
use crate::error::BkeError;
use crate::id::{recalc, truncate_name, IdHeader, IdType, MAX_NAME_BYTES};
use crate::material::{Material, MaterialId};
use crate::mesh::{Mesh, MeshId};
use crate::object::{Object, ObjectData, ObjectId};
use crate::scene::{Scene, SceneId};

/// A pending update, returned by [`Main::take_changes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub id_type: IdType,
    pub handle: u32,
    /// Bits from [`recalc`].
    pub flags: u32,
}

/// Where objects or child collections can be linked: a scene's master collection or a
/// stand-alone collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionRef {
    Scene(SceneId),
    Collection(CollectionId),
}

struct Arena<T> {
    slots: Vec<Option<T>>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena { slots: Vec::new() }
    }
}

impl<T> Arena<T> {
    fn insert(&mut self, value: T) -> u32 {
        self.slots.push(Some(value));
        (self.slots.len() - 1) as u32
    }

    fn get(&self, handle: u32) -> Option<&T> {
        self.slots.get(handle as usize)?.as_ref()
    }

    fn get_mut(&mut self, handle: u32) -> Option<&mut T> {
        self.slots.get_mut(handle as usize)?.as_mut()
    }

    fn remove(&mut self, handle: u32) -> Option<T> {
        self.slots.get_mut(handle as usize)?.take()
    }

    fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| Some((i as u32, slot.as_ref()?)))
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = (u32, &mut T)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(i, slot)| Some((i as u32, slot.as_mut()?)))
    }
}

trait DataBlock {
    fn header_mut(&mut self) -> &mut IdHeader;
}

impl DataBlock for Scene {
    fn header_mut(&mut self) -> &mut IdHeader {
        &mut self.id
    }
}

impl DataBlock for Object {
    fn header_mut(&mut self) -> &mut IdHeader {
        &mut self.id
    }
}

impl DataBlock for Mesh {
    fn header_mut(&mut self) -> &mut IdHeader {
        &mut self.id
    }
}

impl DataBlock for Material {
    fn header_mut(&mut self) -> &mut IdHeader {
        &mut self.id
    }
}

impl DataBlock for Collection {
    fn header_mut(&mut self) -> &mut IdHeader {
        &mut self.id
    }
}

/// Unique names per ID type (`BKE_main_namemap_get_unique_name`):
/// "Cube", then "Cube.001", "Cube.002", ...
#[derive(Default)]
struct NameMap {
    used: HashMap<IdType, HashSet<String>>,
}

impl NameMap {
    fn claim(&mut self, id_type: IdType, requested: &str) -> String {
        let requested = truncate_name(requested, MAX_NAME_BYTES);
        let requested = if requested.is_empty() { default_name(id_type) } else { requested };
        let used = self.used.entry(id_type).or_default();
        if used.insert(requested.to_string()) {
            return requested.to_string();
        }
        let base = strip_number_suffix(requested);
        for number in 1u32.. {
            let suffix = format!(".{number:03}");
            let name = format!("{}{suffix}", truncate_name(base, MAX_NAME_BYTES - suffix.len()));
            if used.insert(name.clone()) {
                return name;
            }
        }
        unreachable!("ran out of name suffixes")
    }

    fn release(&mut self, id_type: IdType, name: &str) {
        if let Some(used) = self.used.get_mut(&id_type) {
            used.remove(name);
        }
    }
}

fn default_name(id_type: IdType) -> &'static str {
    match id_type {
        IdType::Scene => "Scene",
        IdType::Object => "Object",
        IdType::Mesh => "Mesh",
        IdType::Collection => "Collection",
        IdType::Material => "Material",
    }
}

/// "Cube.001" -> "Cube"; names without a numeric suffix are returned unchanged.
fn strip_number_suffix(name: &str) -> &str {
    if let Some((base, digits)) = name.rsplit_once('.') {
        if !base.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return base;
        }
    }
    name
}

fn invalid(id_type: IdType, handle: u32) -> BkeError {
    BkeError::InvalidHandle { id_type, handle }
}

/// All data-blocks of one session. Edit data-blocks through `Main` so user counts, unique
/// names and update flags stay consistent; don't modify the `id` headers directly.
#[derive(Default)]
pub struct Main {
    scenes: Arena<Scene>,
    objects: Arena<Object>,
    meshes: Arena<Mesh>,
    collections: Arena<Collection>,
    materials: Arena<Material>,
    names: NameMap,
    last_session_uid: u32,
}

impl Main {
    pub fn new() -> Main {
        Main::default()
    }

    /// `BKE_libblock_alloc`: unique name, fresh session UID, tagged for a full update.
    fn new_id(&mut self, id_type: IdType, name: &str) -> IdHeader {
        self.last_session_uid += 1;
        IdHeader {
            name: self.names.claim(id_type, name),
            users: 0,
            recalc: recalc::ALL,
            session_uid: self.last_session_uid,
        }
    }

    // ---------------------------------------------------------------- Scenes

    /// `BKE_scene_add`. The scene owns an empty master collection.
    pub fn scene_add(&mut self, name: &str) -> SceneId {
        let id = self.new_id(IdType::Scene, name);
        let master_collection = Collection {
            id: IdHeader { name: "Scene Collection".into(), ..IdHeader::default() },
            ..Collection::default()
        };
        SceneId(self.scenes.insert(Scene { id, master_collection, camera: None }))
    }

    pub fn scene(&self, scene: SceneId) -> Result<&Scene, BkeError> {
        self.scenes.get(scene.0).ok_or(invalid(IdType::Scene, scene.0))
    }

    pub fn scenes(&self) -> impl Iterator<Item = (SceneId, &Scene)> {
        self.scenes.iter().map(|(h, s)| (SceneId(h), s))
    }

    /// Set or clear the active camera object.
    pub fn scene_set_camera(&mut self, scene: SceneId, camera: Option<ObjectId>) -> Result<(), BkeError> {
        if let Some(camera) = camera {
            self.object(camera)?;
        }
        let found = self.scenes.get_mut(scene.0).ok_or(invalid(IdType::Scene, scene.0))?;
        found.camera = camera;
        found.id.recalc |= recalc::RELATIONS;
        Ok(())
    }

    /// Remove a scene; objects and collections linked to it stay in `Main`.
    pub fn scene_remove(&mut self, scene: SceneId) -> Result<(), BkeError> {
        let removed = self.scenes.remove(scene.0).ok_or(invalid(IdType::Scene, scene.0))?;
        self.names.release(IdType::Scene, &removed.id.name);
        self.release_collection_contents(&removed.master_collection);
        Ok(())
    }

    /// Every object in the scene, from the master collection and all nested collections,
    /// each listed once.
    pub fn scene_objects(&self, scene: SceneId) -> Result<Vec<ObjectId>, BkeError> {
        let scene = self.scene(scene)?;
        let mut objects = Vec::new();
        let mut seen_objects = HashSet::new();
        let mut seen_collections = HashSet::new();
        self.collect_objects(&scene.master_collection, &mut objects, &mut seen_objects, &mut seen_collections);
        Ok(objects)
    }

    fn collect_objects(
        &self,
        collection: &Collection,
        objects: &mut Vec<ObjectId>,
        seen_objects: &mut HashSet<ObjectId>,
        seen_collections: &mut HashSet<CollectionId>,
    ) {
        for &object in &collection.objects {
            if seen_objects.insert(object) {
                objects.push(object);
            }
        }
        for &child in &collection.children {
            if seen_collections.insert(child) {
                if let Some(child) = self.collections.get(child.0) {
                    self.collect_objects(child, objects, seen_objects, seen_collections);
                }
            }
        }
    }

    // ---------------------------------------------------------------- Meshes

    /// Add a mesh (from [`crate::primitives`] or [`Mesh::from_faces`]) to the database.
    pub fn mesh_add(&mut self, name: &str, mut mesh: Mesh) -> MeshId {
        mesh.id = self.new_id(IdType::Mesh, name);
        MeshId(self.meshes.insert(mesh))
    }

    pub fn mesh(&self, mesh: MeshId) -> Result<&Mesh, BkeError> {
        self.meshes.get(mesh.0).ok_or(invalid(IdType::Mesh, mesh.0))
    }

    /// Mutable access; tags the mesh for a geometry update.
    pub fn mesh_mut(&mut self, mesh: MeshId) -> Result<&mut Mesh, BkeError> {
        let found = self.meshes.get_mut(mesh.0).ok_or(invalid(IdType::Mesh, mesh.0))?;
        found.id.recalc |= recalc::GEOMETRY;
        Ok(found)
    }

    pub fn meshes(&self) -> impl Iterator<Item = (MeshId, &Mesh)> {
        self.meshes.iter().map(|(h, m)| (MeshId(h), m))
    }

    /// Remove a mesh. Refused while objects still use it.
    pub fn mesh_remove(&mut self, mesh: MeshId) -> Result<(), BkeError> {
        let users = self.mesh(mesh)?.id.users;
        if users > 0 {
            return Err(BkeError::StillInUse { id_type: IdType::Mesh, handle: mesh.0, users });
        }
        let removed = self.meshes.remove(mesh.0).expect("checked above");
        self.names.release(IdType::Mesh, &removed.id.name);
        for material in removed.materials.iter().flatten() {
            self.remove_material_user(*material);
        }
        Ok(())
    }

    /// Replace a mesh's material slots (`BKE_id_material_assign`). Each slot holding a
    /// material counts as one user of it.
    pub fn mesh_set_materials(&mut self, mesh: MeshId, materials: Vec<Option<MaterialId>>) -> Result<(), BkeError> {
        self.mesh(mesh)?;
        for material in materials.iter().flatten() {
            self.material(*material)?;
        }
        for material in materials.iter().flatten() {
            self.materials.get_mut(material.0).expect("checked above").id.users += 1;
        }
        let found = self.meshes.get_mut(mesh.0).expect("checked above");
        let old = std::mem::replace(&mut found.materials, materials);
        found.id.recalc |= recalc::SHADING | recalc::RELATIONS;
        for material in old.iter().flatten() {
            self.remove_material_user(*material);
        }
        Ok(())
    }

    // ---------------------------------------------------------------- Materials

    /// `BKE_material_add`. With `use_nodes`, the material gets Blender's default surface tree
    /// (Principled BSDF → Material Output), as the "New Material" button creates.
    pub fn material_add(&mut self, name: &str, use_nodes: bool) -> MaterialId {
        let mut material = if use_nodes { Material::with_default_nodes() } else { Material::new() };
        material.id = self.new_id(IdType::Material, name);
        MaterialId(self.materials.insert(material))
    }

    pub fn material(&self, material: MaterialId) -> Result<&Material, BkeError> {
        self.materials.get(material.0).ok_or(invalid(IdType::Material, material.0))
    }

    /// Mutable access to values and the node tree; tags the material for a shading update.
    pub fn material_mut(&mut self, material: MaterialId) -> Result<&mut Material, BkeError> {
        let found = self
            .materials
            .get_mut(material.0)
            .ok_or(invalid(IdType::Material, material.0))?;
        found.id.recalc |= recalc::SHADING;
        Ok(found)
    }

    pub fn materials(&self) -> impl Iterator<Item = (MaterialId, &Material)> {
        self.materials.iter().map(|(h, m)| (MaterialId(h), m))
    }

    /// Remove a material. Refused while mesh slots still use it.
    pub fn material_remove(&mut self, material: MaterialId) -> Result<(), BkeError> {
        let users = self.material(material)?.id.users;
        if users > 0 {
            return Err(BkeError::StillInUse { id_type: IdType::Material, handle: material.0, users });
        }
        let removed = self.materials.remove(material.0).expect("checked above");
        self.names.release(IdType::Material, &removed.id.name);
        Ok(())
    }

    /// The material an object's face uses: the mesh's slot `slot`, clamped like Blender
    /// (`BKE_object_material_get`). `None` for empties, meshes without slots, or empty slots.
    pub fn object_material(&self, object: ObjectId, slot: usize) -> Result<Option<MaterialId>, BkeError> {
        let Some(mesh) = self.object(object)?.mesh() else {
            return Ok(None);
        };
        let slots = &self.mesh(mesh)?.materials;
        if slots.is_empty() {
            return Ok(None);
        }
        Ok(slots[slot.min(slots.len() - 1)])
    }

    fn remove_material_user(&mut self, material: MaterialId) {
        if let Some(found) = self.materials.get_mut(material.0) {
            found.id.users = found.id.users.saturating_sub(1);
        }
    }

    // ---------------------------------------------------------------- Objects

    /// `BKE_object_add_only_object` + assigning `data`. The object is not linked to any
    /// collection yet; use [`Main::collection_link_object`].
    pub fn object_add(&mut self, name: &str, data: ObjectData) -> Result<ObjectId, BkeError> {
        if let ObjectData::Mesh(mesh) = data {
            self.mesh(mesh)?;
        }
        let mut object = Object::new(data);
        object.id = self.new_id(IdType::Object, name);
        self.add_data_user(data);
        Ok(ObjectId(self.objects.insert(object)))
    }

    pub fn object(&self, object: ObjectId) -> Result<&Object, BkeError> {
        self.objects.get(object.0).ok_or(invalid(IdType::Object, object.0))
    }

    /// Mutable access for transform edits; tags the object for a transform update.
    pub fn object_mut(&mut self, object: ObjectId) -> Result<&mut Object, BkeError> {
        let found = self.objects.get_mut(object.0).ok_or(invalid(IdType::Object, object.0))?;
        found.id.recalc |= recalc::TRANSFORM;
        Ok(found)
    }

    pub fn objects(&self) -> impl Iterator<Item = (ObjectId, &Object)> {
        self.objects.iter().map(|(h, o)| (ObjectId(h), o))
    }

    /// Change what the object instances, keeping mesh user counts right.
    pub fn object_set_data(&mut self, object: ObjectId, data: ObjectData) -> Result<(), BkeError> {
        if let ObjectData::Mesh(mesh) = data {
            self.mesh(mesh)?;
        }
        let found = self.objects.get_mut(object.0).ok_or(invalid(IdType::Object, object.0))?;
        let old = std::mem::replace(&mut found.data, data);
        found.id.recalc |= recalc::GEOMETRY | recalc::RELATIONS;
        self.add_data_user(data);
        self.remove_data_user(old);
        Ok(())
    }

    /// Set or clear the parent. Refuses parents that would create a cycle.
    /// The child's local transform is kept (no parent-inverse matrix in this port).
    pub fn object_set_parent(&mut self, child: ObjectId, parent: Option<ObjectId>) -> Result<(), BkeError> {
        self.object(child)?;
        if let Some(parent) = parent {
            self.object(parent)?;
            let mut ancestor = Some(parent);
            while let Some(current) = ancestor {
                if current == child {
                    return Err(BkeError::ParentCycle);
                }
                ancestor = self.objects.get(current.0).and_then(|o| o.parent);
            }
        }
        let found = self.objects.get_mut(child.0).expect("checked above");
        found.parent = parent;
        found.id.recalc |= recalc::TRANSFORM | recalc::RELATIONS;
        Ok(())
    }

    /// Delete an object: unlink it from every collection, clear it as parent and camera,
    /// and release its mesh.
    pub fn object_remove(&mut self, object: ObjectId) -> Result<(), BkeError> {
        let removed = self.objects.remove(object.0).ok_or(invalid(IdType::Object, object.0))?;
        self.names.release(IdType::Object, &removed.id.name);
        self.remove_data_user(removed.data);
        for (_, scene) in self.scenes.iter_mut() {
            if scene.master_collection.remove_object(object) {
                scene.id.recalc |= recalc::RELATIONS;
            }
            if scene.camera == Some(object) {
                scene.camera = None;
            }
        }
        for (_, collection) in self.collections.iter_mut() {
            if collection.remove_object(object) {
                collection.id.recalc |= recalc::RELATIONS;
            }
        }
        for (_, other) in self.objects.iter_mut() {
            if other.parent == Some(object) {
                other.parent = None;
                other.id.recalc |= recalc::TRANSFORM | recalc::RELATIONS;
            }
        }
        Ok(())
    }

    /// Transform relative to the parent (`BKE_object_to_mat4`), column-major.
    pub fn object_local_matrix(&self, object: ObjectId) -> Result<Mat4, BkeError> {
        Ok(self.object(object)?.local_matrix())
    }

    /// Transform in world space: parent world matrix × local matrix, up the parent chain.
    pub fn object_world_matrix(&self, object: ObjectId) -> Result<Mat4, BkeError> {
        let found = self.object(object)?;
        let mut matrix = found.local_matrix();
        let mut ancestor = found.parent;
        while let Some(parent) = ancestor {
            let parent = self.object(parent)?;
            matrix = parent.local_matrix() * matrix;
            ancestor = parent.parent;
        }
        Ok(matrix)
    }

    fn add_data_user(&mut self, data: ObjectData) {
        if let ObjectData::Mesh(mesh) = data {
            if let Some(mesh) = self.meshes.get_mut(mesh.0) {
                mesh.id.users += 1;
            }
        }
    }

    fn remove_data_user(&mut self, data: ObjectData) {
        if let ObjectData::Mesh(mesh) = data {
            if let Some(mesh) = self.meshes.get_mut(mesh.0) {
                mesh.id.users = mesh.id.users.saturating_sub(1);
            }
        }
    }

    // ---------------------------------------------------------------- Collections

    pub fn collection_add(&mut self, name: &str) -> CollectionId {
        let id = self.new_id(IdType::Collection, name);
        CollectionId(self.collections.insert(Collection { id, ..Collection::default() }))
    }

    pub fn collection(&self, collection: CollectionId) -> Result<&Collection, BkeError> {
        self.collections.get(collection.0).ok_or(invalid(IdType::Collection, collection.0))
    }

    pub fn collections(&self) -> impl Iterator<Item = (CollectionId, &Collection)> {
        self.collections.iter().map(|(h, c)| (CollectionId(h), c))
    }

    fn collection_ref(&self, target: CollectionRef) -> Result<&Collection, BkeError> {
        match target {
            CollectionRef::Scene(scene) => Ok(&self.scene(scene)?.master_collection),
            CollectionRef::Collection(collection) => self.collection(collection),
        }
    }

    fn collection_ref_mut(&mut self, target: CollectionRef) -> Result<&mut Collection, BkeError> {
        match target {
            CollectionRef::Scene(scene) => self
                .scenes
                .get_mut(scene.0)
                .map(|s| &mut s.master_collection)
                .ok_or(invalid(IdType::Scene, scene.0)),
            CollectionRef::Collection(collection) => self
                .collections
                .get_mut(collection.0)
                .ok_or(invalid(IdType::Collection, collection.0)),
        }
    }

    /// Tag the data-block that owns `target` (the scene for a master collection).
    fn tag_collection_ref(&mut self, target: CollectionRef, flags: u32) {
        match target {
            CollectionRef::Scene(scene) => {
                if let Some(scene) = self.scenes.get_mut(scene.0) {
                    scene.id.recalc |= flags;
                }
            }
            CollectionRef::Collection(collection) => {
                if let Some(collection) = self.collections.get_mut(collection.0) {
                    collection.id.recalc |= flags;
                }
            }
        }
    }

    /// `BKE_collection_object_add`. Returns false if it was already linked there.
    /// Each link counts as one user of the object.
    pub fn collection_link_object(&mut self, target: CollectionRef, object: ObjectId) -> Result<bool, BkeError> {
        self.object(object)?;
        let added = self.collection_ref_mut(target)?.add_object(object);
        if added {
            self.objects.get_mut(object.0).expect("checked above").id.users += 1;
            self.tag_collection_ref(target, recalc::RELATIONS);
        }
        Ok(added)
    }

    /// `BKE_collection_object_remove`. Returns false if it wasn't linked there.
    pub fn collection_unlink_object(&mut self, target: CollectionRef, object: ObjectId) -> Result<bool, BkeError> {
        let removed = self.collection_ref_mut(target)?.remove_object(object);
        if removed {
            if let Some(found) = self.objects.get_mut(object.0) {
                found.id.users = found.id.users.saturating_sub(1);
            }
            self.tag_collection_ref(target, recalc::RELATIONS);
        }
        Ok(removed)
    }

    /// `BKE_collection_child_add`. Refuses links that would make a collection contain itself.
    pub fn collection_link_child(&mut self, target: CollectionRef, child: CollectionId) -> Result<bool, BkeError> {
        self.collection(child)?;
        self.collection_ref(target)?;
        if let CollectionRef::Collection(parent) = target {
            if parent == child || self.collection_contains(child, parent) {
                return Err(BkeError::CollectionCycle);
            }
        }
        let added = self.collection_ref_mut(target)?.add_child(child);
        if added {
            self.collections.get_mut(child.0).expect("checked above").id.users += 1;
            self.tag_collection_ref(target, recalc::RELATIONS);
        }
        Ok(added)
    }

    /// Does `collection` contain `needle` anywhere below it?
    fn collection_contains(&self, collection: CollectionId, needle: CollectionId) -> bool {
        let mut stack = vec![collection];
        let mut seen = HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current) {
                continue;
            }
            if let Some(found) = self.collections.get(current.0) {
                for &child in &found.children {
                    if child == needle {
                        return true;
                    }
                    stack.push(child);
                }
            }
        }
        false
    }

    /// Delete a collection: unlink it from its parents and release its objects and children.
    pub fn collection_remove(&mut self, collection: CollectionId) -> Result<(), BkeError> {
        let removed = self
            .collections
            .remove(collection.0)
            .ok_or(invalid(IdType::Collection, collection.0))?;
        self.names.release(IdType::Collection, &removed.id.name);
        self.release_collection_contents(&removed);
        for (_, scene) in self.scenes.iter_mut() {
            if scene.master_collection.remove_child(collection) {
                scene.id.recalc |= recalc::RELATIONS;
            }
        }
        for (_, other) in self.collections.iter_mut() {
            if other.remove_child(collection) {
                other.id.recalc |= recalc::RELATIONS;
            }
        }
        Ok(())
    }

    /// Drop the user each link of a removed collection held on its objects and children.
    fn release_collection_contents(&mut self, removed: &Collection) {
        for object in &removed.objects {
            if let Some(found) = self.objects.get_mut(object.0) {
                found.id.users = found.id.users.saturating_sub(1);
                found.id.recalc |= recalc::RELATIONS;
            }
        }
        for child in &removed.children {
            if let Some(found) = self.collections.get_mut(child.0) {
                found.id.users = found.id.users.saturating_sub(1);
            }
        }
    }

    // ---------------------------------------------------------------- Updates

    /// Add update flags to a data-block (`DEG_id_tag_update`).
    pub fn tag(&mut self, id_type: IdType, handle: u32, flags: u32) -> Result<(), BkeError> {
        let header = match id_type {
            IdType::Scene => self.scenes.get_mut(handle).map(|s| &mut s.id),
            IdType::Object => self.objects.get_mut(handle).map(|o| &mut o.id),
            IdType::Mesh => self.meshes.get_mut(handle).map(|m| &mut m.id),
            IdType::Collection => self.collections.get_mut(handle).map(|c| &mut c.id),
            IdType::Material => self.materials.get_mut(handle).map(|m| &mut m.id),
        };
        header.ok_or(invalid(id_type, handle))?.recalc |= flags;
        Ok(())
    }

    /// Return every data-block with pending update flags and clear them. The depsgraph calls
    /// this once per frame. Order: scenes, objects, meshes, collections, materials.
    pub fn take_changes(&mut self) -> Vec<Change> {
        fn drain<T: DataBlock>(arena: &mut Arena<T>, id_type: IdType, out: &mut Vec<Change>) {
            for (handle, block) in arena.iter_mut() {
                let header = block.header_mut();
                if header.recalc != 0 {
                    out.push(Change { id_type, handle, flags: header.recalc });
                    header.recalc = 0;
                }
            }
        }
        let mut changes = Vec::new();
        drain(&mut self.scenes, IdType::Scene, &mut changes);
        drain(&mut self.objects, IdType::Object, &mut changes);
        drain(&mut self.meshes, IdType::Mesh, &mut changes);
        drain(&mut self.collections, IdType::Collection, &mut changes);
        drain(&mut self.materials, IdType::Material, &mut changes);
        changes
    }
}
