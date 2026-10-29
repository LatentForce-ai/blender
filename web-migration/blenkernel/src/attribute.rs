//! Generic named attributes on mesh domains, a simplified `AttributeStorage`
//! (`blenkernel/intern/attribute_storage.cc`).
//!
//! The built-in topology arrays (`position`, `.edge_verts`, `.corner_vert`, `.corner_edge`) are
//! stored as typed fields on [`crate::Mesh`] instead, so their names are reserved here.

use crate::error::BkeError;

/// Which element an attribute stores one value per (`bke::AttrDomain`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrDomain {
    Point,
    Edge,
    Face,
    Corner,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttributeData {
    Bool(Vec<bool>),
    Int32(Vec<i32>),
    Float(Vec<f32>),
    Float2(Vec<[f32; 2]>),
    Float3(Vec<[f32; 3]>),
    ColorFloat(Vec<[f32; 4]>),
}

impl AttributeData {
    pub fn len(&self) -> usize {
        match self {
            AttributeData::Bool(v) => v.len(),
            AttributeData::Int32(v) => v.len(),
            AttributeData::Float(v) => v.len(),
            AttributeData::Float2(v) => v.len(),
            AttributeData::Float3(v) => v.len(),
            AttributeData::ColorFloat(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub domain: AttrDomain,
    pub data: AttributeData,
}

/// Names Blender reserves for mesh topology (`mesh.cc`, `is_builtin_mesh_attribute`).
pub const BUILTIN_MESH_ATTRIBUTES: [&str; 4] =
    ["position", ".edge_verts", ".corner_vert", ".corner_edge"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributeStorage {
    attributes: Vec<Attribute>,
}

impl AttributeStorage {
    pub fn lookup(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Attribute> {
        self.attributes.iter()
    }

    pub fn len(&self) -> usize {
        self.attributes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    /// Add an attribute. The caller (the mesh) has already checked its length.
    pub(crate) fn add(&mut self, attribute: Attribute) -> Result<(), BkeError> {
        if BUILTIN_MESH_ATTRIBUTES.contains(&attribute.name.as_str())
            || self.lookup(&attribute.name).is_some()
        {
            return Err(BkeError::AttributeNameTaken(attribute.name));
        }
        self.attributes.push(attribute);
        Ok(())
    }

    pub(crate) fn remove(&mut self, name: &str) -> bool {
        let len_before = self.attributes.len();
        self.attributes.retain(|a| a.name != name);
        self.attributes.len() != len_before
    }
}
