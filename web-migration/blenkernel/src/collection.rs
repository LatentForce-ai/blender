//! Port of `struct Collection` (`makesdna/DNA_collection_types.h`,
//! `blenkernel/intern/collection.cc`). `ListBase`s of `CollectionObject` / `CollectionChild`
//! become plain vectors of handles.

use crate::id::IdHeader;
use crate::object::ObjectId;

/// Handle of a collection in a [`crate::Main`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CollectionId(pub u32);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collection {
    pub id: IdHeader,
    pub(crate) objects: Vec<ObjectId>,
    pub(crate) children: Vec<CollectionId>,
}

impl Collection {
    pub fn objects(&self) -> &[ObjectId] {
        &self.objects
    }

    pub fn children(&self) -> &[CollectionId] {
        &self.children
    }

    /// Returns false if the object was already linked (`BKE_collection_object_add`).
    pub(crate) fn add_object(&mut self, object: ObjectId) -> bool {
        if self.objects.contains(&object) {
            return false;
        }
        self.objects.push(object);
        true
    }

    pub(crate) fn remove_object(&mut self, object: ObjectId) -> bool {
        let len_before = self.objects.len();
        self.objects.retain(|&o| o != object);
        self.objects.len() != len_before
    }

    pub(crate) fn add_child(&mut self, child: CollectionId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        self.children.push(child);
        true
    }

    pub(crate) fn remove_child(&mut self, child: CollectionId) -> bool {
        let len_before = self.children.len();
        self.children.retain(|&c| c != child);
        self.children.len() != len_before
    }
}
