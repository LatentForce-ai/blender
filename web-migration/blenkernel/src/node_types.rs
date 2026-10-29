//! Shader node type registry: the socket declarations of the node types this port supports,
//! copied from `nodes/shader/nodes/node_shader_*.cc` (`node_declare`).
//!
//! Only the sockets needed for basic materials are declared; e.g. the Principled BSDF lists its
//! main inputs, not every subsurface/thin-film/anisotropy one. Identifiers match Blender's, so
//! files and scripts can refer to the same socket names.

use crate::node::{SocketType, SocketValue};

pub struct SocketDecl {
    pub identifier: &'static str,
    pub name: &'static str,
    pub socket_type: SocketType,
    pub default_value: SocketValue,
}

pub struct NodeDecl {
    pub idname: &'static str,
    /// Default node name, as shown in Blender's node editor.
    pub label: &'static str,
    pub inputs: Vec<SocketDecl>,
    pub outputs: Vec<SocketDecl>,
    /// Node settings that are not sockets (`bNode::storage`, enum properties), as strings.
    pub properties: Vec<(&'static str, &'static str)>,
}

/// Idnames of all supported node types.
pub const NODE_TYPES: [&str; 10] = [
    "ShaderNodeOutputMaterial",
    "ShaderNodeBsdfPrincipled",
    "ShaderNodeBsdfDiffuse",
    "ShaderNodeEmission",
    "ShaderNodeMixShader",
    "ShaderNodeMix",
    "ShaderNodeMath",
    "ShaderNodeRGB",
    "ShaderNodeValue",
    "ShaderNodeTexImage",
];

fn sock(identifier: &'static str, name: &'static str, socket_type: SocketType, default_value: SocketValue) -> SocketDecl {
    SocketDecl { identifier, name, socket_type, default_value }
}

fn float(name: &'static str, value: f32) -> SocketDecl {
    sock(name, name, SocketType::Float, SocketValue::Float(value))
}

fn color(name: &'static str, value: [f32; 4]) -> SocketDecl {
    sock(name, name, SocketType::Color, SocketValue::Color(value))
}

fn vector(name: &'static str) -> SocketDecl {
    sock(name, name, SocketType::Vector, SocketValue::Vector([0.0; 3]))
}

fn shader(name: &'static str) -> SocketDecl {
    sock(name, name, SocketType::Shader, SocketValue::None)
}

const GREY_08: [f32; 4] = [0.8, 0.8, 0.8, 1.0];
const GREY_05: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Declaration of a node type, or `None` if the type is not supported.
pub fn declaration(idname: &str) -> Option<NodeDecl> {
    let (label, inputs, outputs, properties) = match idname {
        // node_shader_output_material.cc
        "ShaderNodeOutputMaterial" => (
            "Material Output",
            vec![shader("Surface"), shader("Volume"), vector("Displacement"), float("Thickness", 0.0)],
            vec![],
            vec![("target", "ALL")],
        ),
        // node_shader_bsdf_principled.cc (main inputs)
        "ShaderNodeBsdfPrincipled" => (
            "Principled BSDF",
            vec![
                color("Base Color", GREY_08),
                float("Metallic", 0.0),
                float("Roughness", 0.5),
                float("IOR", 1.5),
                float("Alpha", 1.0),
                vector("Normal"),
                float("Subsurface Weight", 0.0),
                float("Specular IOR Level", 0.5),
                color("Specular Tint", WHITE),
                float("Transmission Weight", 0.0),
                float("Coat Weight", 0.0),
                float("Sheen Weight", 0.0),
                color("Emission Color", WHITE),
                float("Emission Strength", 0.0),
            ],
            vec![shader("BSDF")],
            vec![("distribution", "MULTI_GGX"), ("subsurface_method", "RANDOM_WALK")],
        ),
        // node_shader_bsdf_diffuse.cc
        "ShaderNodeBsdfDiffuse" => (
            "Diffuse BSDF",
            vec![color("Color", GREY_08), float("Roughness", 0.0), vector("Normal")],
            vec![shader("BSDF")],
            vec![],
        ),
        // node_shader_emission.cc
        "ShaderNodeEmission" => (
            "Emission",
            vec![color("Color", WHITE), float("Strength", 1.0)],
            vec![shader("Emission")],
            vec![],
        ),
        // node_shader_mix_shader.cc
        "ShaderNodeMixShader" => (
            "Mix Shader",
            vec![
                sock("Fac", "Factor", SocketType::Float, SocketValue::Float(0.5)),
                shader("Shader"),
                sock("Shader_001", "Shader", SocketType::Shader, SocketValue::None),
            ],
            vec![shader("Shader")],
            vec![],
        ),
        // node_shader_mix.cc, color mode only (data_type RGBA)
        "ShaderNodeMix" => (
            "Mix",
            vec![
                sock("Factor_Float", "Factor", SocketType::Float, SocketValue::Float(1.0)),
                sock("A_Color", "A", SocketType::Color, SocketValue::Color(GREY_05)),
                sock("B_Color", "B", SocketType::Color, SocketValue::Color(GREY_05)),
            ],
            vec![sock("Result_Color", "Result", SocketType::Color, SocketValue::Color([0.0, 0.0, 0.0, 1.0]))],
            vec![("data_type", "RGBA"), ("blend_type", "MIX"), ("clamp_result", "false")],
        ),
        // node_shader_math.cc
        "ShaderNodeMath" => (
            "Math",
            vec![
                float("Value", 0.5),
                sock("Value_001", "Value", SocketType::Float, SocketValue::Float(0.5)),
                sock("Value_002", "Value", SocketType::Float, SocketValue::Float(0.5)),
            ],
            vec![float("Value", 0.0)],
            vec![("operation", "ADD"), ("use_clamp", "false")],
        ),
        // node_shader_rgb.cc: the color is the output socket's default value.
        "ShaderNodeRGB" => ("RGB", vec![], vec![color("Color", GREY_05)], vec![]),
        // node_shader_value.cc: the value is the output socket's default value.
        "ShaderNodeValue" => ("Value", vec![], vec![float("Value", 0.5)], vec![]),
        // node_shader_tex_image.cc; `image` holds an image name/path (no Image data-block yet).
        "ShaderNodeTexImage" => (
            "Image Texture",
            vec![vector("Vector")],
            vec![color("Color", [0.0, 0.0, 0.0, 1.0]), float("Alpha", 0.0)],
            vec![("image", ""), ("interpolation", "Linear"), ("projection", "FLAT"), ("extension", "REPEAT")],
        ),
        _ => return None,
    };
    Some(NodeDecl {
        idname: NODE_TYPES.into_iter().find(|&t| t == idname)?,
        label,
        inputs,
        outputs,
        properties,
    })
}
