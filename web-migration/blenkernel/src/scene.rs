//! Port of `struct Scene` (`makesdna/DNA_scene_types.h`, `blenkernel/intern/scene.cc`).
//! Only the master collection and the active camera are kept.

use crate::collection::Collection;
use crate::id::IdHeader;
use crate::object::ObjectId;

/// Handle of a scene in a [`crate::Main`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneId(pub u32);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub id: IdHeader,
    /// Embedded root collection ("Scene Collection"), owned by the scene as in Blender.
    pub(crate) master_collection: Collection,
    pub camera: Option<ObjectId>,
}

impl Scene {
    pub fn master_collection(&self) -> &Collection {
        &self.master_collection
    }
}
