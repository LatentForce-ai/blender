//! Thin Rust port of Blender's dependency graph (`source/blender/depsgraph`), on top of the
//! blenkernel port in `../blenkernel`.
//!
//! The depsgraph answers one question each frame: "something was edited, what else is now out of
//! date?" Move a parent and its children move too; edit a mesh and every object using it changes.
//! Only those are recomputed, and render is told exactly which ones changed.
//!
//! Each [`Depsgraph::evaluate`] call follows the C++ pipeline:
//!
//! 1. **Tag**: read the edits blenkernel recorded with [`Main::take_changes`]
//!    (`DEG_id_tag_update`, `depsgraph_tag.cc`).
//! 2. **Build relations** when links changed: which objects are in the scene, who is whose parent,
//!    which objects use which mesh (`DEG_graph_relations_update`, `depsgraph_build.cc`).
//! 3. **Flush**: a moved object makes all its descendants out of date too
//!    (`deg_graph_flush_updates`, `eval/deg_eval_flush.cc`).
//! 4. **Evaluate** what is out of date, parents before children (`deg_evaluate_on_refresh`,
//!    `eval/deg_eval.cc`): world matrices for objects, derived data for meshes.
//!
//! Scope matches the blenkernel slice: object transforms, parenting, meshes, collections. No
//! animation, drivers, constraints or modifiers, and no copy-on-evaluation: results are cached
//! here (world matrices) or in the blenkernel mesh caches, not in evaluated copies of the scene.

use std::collections::{HashMap, HashSet};

use blenkernel::{recalc, BkeError, IdType, Main, MeshId, ObjectId, SceneId};
use glam::Mat4;

/// What one [`Depsgraph::evaluate`] call recomputed. Render re-uploads only these.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Updates {
    /// Objects whose world matrix was recomputed, parents before children. May include parents
    /// that are not in the scene themselves but whose children are.
    pub transforms: Vec<ObjectId>,
    /// Meshes whose derived data (normals, triangles, bounds) was recomputed, by handle.
    pub geometry: Vec<MeshId>,
    /// Objects were linked or unlinked, re-parented or given other data: refresh the draw list
    /// from [`Depsgraph::objects`].
    pub relations: bool,
    /// Objects that left the scene since the last evaluation.
    pub removed: Vec<ObjectId>,
}

impl Updates {
    pub fn is_empty(&self) -> bool {
        *self == Updates::default()
    }
}

/// The dependency graph of one scene.
///
/// [`Main::take_changes`] clears the edits of *every* data-block, so use one depsgraph per `Main`.
// ponytail: one depsgraph per Main; per-graph tag lists (like C++ `Depsgraph::id_tags`) if
// several scenes ever need their own graphs.
pub struct Depsgraph {
    scene: SceneId,
    /// Objects of the scene, in `Main::scene_objects` order: what render draws.
    objects: Vec<ObjectId>,
    /// Every object whose matrix is evaluated (scene objects plus their parents) and its parent.
    parents: HashMap<ObjectId, Option<ObjectId>>,
    /// Evaluation order: parents before children.
    order: Vec<ObjectId>,
    children: HashMap<ObjectId, Vec<ObjectId>>,
    /// Scene objects using each mesh.
    mesh_users: HashMap<MeshId, Vec<ObjectId>>,
    /// Evaluated world matrices, column-major like blenkernel's.
    world_matrices: HashMap<ObjectId, Mat4>,
    needs_relations: bool,
}

impl Depsgraph {
    /// `DEG_graph_new`. Nothing is built until the first [`Depsgraph::evaluate`].
    pub fn new(scene: SceneId) -> Depsgraph {
        Depsgraph {
            scene,
            objects: Vec::new(),
            parents: HashMap::new(),
            order: Vec::new(),
            children: HashMap::new(),
            mesh_users: HashMap::new(),
            world_matrices: HashMap::new(),
            needs_relations: true,
        }
    }

    pub fn scene(&self) -> SceneId {
        self.scene
    }

    /// Objects in the scene, as of the last evaluation.
    pub fn objects(&self) -> &[ObjectId] {
        &self.objects
    }

    /// Evaluated world matrix, or `None` if the object isn't in the graph.
    pub fn world_matrix(&self, object: ObjectId) -> Option<Mat4> {
        self.world_matrices.get(&object).copied()
    }

    /// Bring everything up to date with the edits made to `bmain` since the last call. Call once
    /// per frame, before drawing.
    pub fn evaluate(&mut self, bmain: &mut Main) -> Result<Updates, BkeError> {
        let changes = bmain.take_changes();
        let mut updates = Updates::default();

        if self.needs_relations || changes.iter().any(|change| change.flags & recalc::RELATIONS != 0) {
            updates.removed = self.build_relations(bmain)?;
            updates.relations = true;
            self.needs_relations = false;
        }

        // Tag. Objects new to the graph have no matrix yet, so they start out of date.
        let mut dirty_objects: HashSet<ObjectId> =
            self.order.iter().copied().filter(|object| !self.world_matrices.contains_key(object)).collect();
        let mut dirty_meshes: HashSet<MeshId> = HashSet::new();
        for &object in &dirty_objects {
            dirty_meshes.extend(bmain.object(object)?.mesh());
        }
        for change in &changes {
            match change.id_type {
                IdType::Object => {
                    let object = ObjectId(change.handle);
                    if !self.parents.contains_key(&object) {
                        continue;
                    }
                    if change.flags & recalc::TRANSFORM != 0 {
                        dirty_objects.insert(object);
                    }
                    if change.flags & recalc::GEOMETRY != 0 {
                        dirty_meshes.extend(bmain.object(object)?.mesh());
                    }
                }
                IdType::Mesh => {
                    let mesh = MeshId(change.handle);
                    if change.flags & recalc::GEOMETRY != 0 && self.mesh_users.contains_key(&mesh) {
                        dirty_meshes.insert(mesh);
                    }
                }
                // Everything else needs nothing here: scenes and collections only carry RELATIONS,
                // handled above; materials (SHADING) are re-checked by the shading crate itself;
                // images are immutable once added. A catch-all so a new data-block type in
                // blenkernel doesn't break this crate's build.
                _ => {}
            }
        }

        // Flush: a moved object moves everything parented to it.
        let mut stack: Vec<ObjectId> = dirty_objects.iter().copied().collect();
        while let Some(object) = stack.pop() {
            for &child in self.children.get(&object).into_iter().flatten() {
                if dirty_objects.insert(child) {
                    stack.push(child);
                }
            }
        }

        // Evaluate transforms. A parent comes first in `order`, so its matrix is already current.
        for &object in &self.order {
            if !dirty_objects.contains(&object) {
                continue;
            }
            let local = bmain.object_local_matrix(object)?;
            let world = match self.parents[&object] {
                Some(parent) => self.world_matrices[&parent] * local,
                None => local,
            };
            self.world_matrices.insert(object, world);
            updates.transforms.push(object);
        }

        // Evaluate geometry: fill the mesh caches render reads, so drawing doesn't pay for them.
        updates.geometry = dirty_meshes.into_iter().collect();
        updates.geometry.sort_by_key(|mesh| mesh.0);
        for &mesh in &updates.geometry {
            let mesh = bmain.mesh(mesh)?;
            mesh.vert_normals();
            mesh.corner_tris();
            mesh.bounds();
        }

        Ok(updates)
    }

    /// Rebuild who-depends-on-whom from scratch, like the C++ builder does on
    /// `DEG_relations_tag_update`. Returns the objects that left the scene.
    fn build_relations(&mut self, bmain: &Main) -> Result<Vec<ObjectId>, BkeError> {
        let objects = bmain.scene_objects(self.scene)?;

        // Pull in parents that aren't in the scene: their matrices still move their children.
        let mut parents = HashMap::new();
        let mut stack = objects.clone();
        while let Some(object) = stack.pop() {
            if parents.contains_key(&object) {
                continue;
            }
            let parent = bmain.object(object)?.parent();
            parents.insert(object, parent);
            stack.extend(parent);
        }

        let mut children: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
        for (&object, &parent) in &parents {
            if let Some(parent) = parent {
                children.entry(parent).or_default().push(object);
            }
        }

        let mut mesh_users: HashMap<MeshId, Vec<ObjectId>> = HashMap::new();
        for &object in &objects {
            if let Some(mesh) = bmain.object(object)?.mesh() {
                mesh_users.entry(mesh).or_default().push(object);
            }
        }

        // Parents before children: walk up from each object to the first one already placed,
        // then place that chain top-down. blenkernel refuses parent cycles, so this ends.
        let mut nodes: Vec<ObjectId> = parents.keys().copied().collect();
        nodes.sort_by_key(|object| object.0);
        let mut order = Vec::with_capacity(nodes.len());
        let mut placed = HashSet::new();
        for start in nodes {
            let mut chain = Vec::new();
            let mut current = Some(start);
            while let Some(object) = current {
                if placed.contains(&object) {
                    break;
                }
                chain.push(object);
                current = parents[&object];
            }
            for object in chain.into_iter().rev() {
                placed.insert(object);
                order.push(object);
            }
        }

        let kept: HashSet<ObjectId> = objects.iter().copied().collect();
        let removed = self.objects.iter().copied().filter(|object| !kept.contains(object)).collect();
        self.world_matrices.retain(|object, _| parents.contains_key(object));

        self.objects = objects;
        self.parents = parents;
        self.order = order;
        self.children = children;
        self.mesh_users = mesh_users;
        Ok(removed)
    }
}
