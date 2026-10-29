//! Port of `struct Material` (`makesdna/DNA_material_types.h`, `DNA_material_defaults.h`,
//! `blenkernel/intern/material.cc`).

use crate::id::IdHeader;
use crate::node::{NodeTree, SocketInOut, SocketValue};

/// Handle of a material in a [`crate::Main`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub id: IdHeader,
    /// Viewport display color (`Material::r, g, b, a`). Solid mode draws this.
    pub base_color: [f32; 4],
    /// Viewport display metallic (`Material::metallic`).
    pub metallic: f32,
    /// Viewport display roughness (`Material::roughness`).
    pub roughness: f32,
    /// Shader node tree (`Material::nodetree`), embedded in the material as in Blender.
    pub node_tree: Option<NodeTree>,
}

impl Default for Material {
    /// Defaults from `DNA_material_defaults.h`: 0.8 grey, roughness 0.4, not metallic.
    fn default() -> Self {
        Material {
            id: IdHeader::default(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.4,
            node_tree: None,
        }
    }
}

impl Material {
    /// A material without nodes.
    pub fn new() -> Material {
        Material::default()
    }

    /// A material with Blender's default surface tree (`material_default_surface_init`):
    /// Principled BSDF → Material Output, Base Color taken from the material color.
    pub fn with_default_nodes() -> Material {
        let mut material = Material::default();
        material.node_tree = Some(default_surface_tree(material.base_color));
        material
    }
}

/// Principled BSDF linked into a Material Output, laid out like Blender's.
pub fn default_surface_tree(base_color: [f32; 4]) -> NodeTree {
    let mut tree = NodeTree::new();
    let principled = tree.add_node("ShaderNodeBsdfPrincipled").expect("built-in type");
    let output = tree.add_node("ShaderNodeOutputMaterial").expect("built-in type");
    tree.set_socket_value(principled, SocketInOut::Input, "Base Color", SocketValue::Color(base_color))
        .expect("declared socket");
    tree.add_link(principled, "BSDF", output, "Surface").expect("valid link");
    tree.set_location(principled, -200.0, 100.0).expect("live node");
    tree.set_location(output, 200.0, 100.0).expect("live node");
    tree
}
