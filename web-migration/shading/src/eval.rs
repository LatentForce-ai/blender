//! Evaluating a material's node tree into [`ShadedMaterial`].
//!
//! Evaluation pulls from the active Material Output's "Surface" input back through the links,
//! caching each node output, the way Blender's GPU codegen walks the tree from the output node
//! (`ntreeGPUMaterialNodes`). Shader sockets carry a [`Closure`]: the BSDF parameters the
//! render pipeline needs, mixed linearly by Mix Shader nodes.

use std::collections::HashMap;
use std::fmt;

use blenkernel::{Main, Material, MaterialId, Node, NodeId, NodeTree};
use serde::Serialize;

use crate::ops;
use crate::value::Value;

/// What the render pipeline draws a material with. Colors are linear RGB.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShadedMaterial {
    pub base_color: [f32; 3],
    pub alpha: f32,
    pub metallic: f32,
    pub roughness: f32,
    /// Emitted radiance (Emission Color × Strength); added on top of lighting.
    pub emission: [f32; 3],
    /// Image name/path of an Image Texture feeding the base color. When set, `base_color` is
    /// the factor to multiply the sampled texture by (white for a direct link).
    pub base_color_texture: Option<String>,
}

impl ShadedMaterial {
    /// Blender's default surface (`BKE_material_default_surface`): used for faces with no
    /// material. A Principled BSDF at its defaults.
    pub const DEFAULT_SURFACE: ShadedMaterial = ShadedMaterial {
        base_color: [0.8, 0.8, 0.8],
        alpha: 1.0,
        metallic: 0.0,
        roughness: 0.5,
        emission: [0.0; 3],
        base_color_texture: None,
    };

    /// What Blender draws for a material whose shader failed to build: magenta.
    pub const ERROR: ShadedMaterial = ShadedMaterial {
        base_color: [1.0, 0.0, 1.0],
        alpha: 1.0,
        metallic: 0.0,
        roughness: 1.0,
        emission: [0.0; 3],
        base_color_texture: None,
    };

    /// No closure: an output with nothing linked to Surface, or an empty Mix Shader input.
    /// Draws black, as in Blender.
    const EMPTY: ShadedMaterial = ShadedMaterial {
        base_color: [0.0; 3],
        alpha: 1.0,
        metallic: 0.0,
        roughness: 1.0,
        emission: [0.0; 3],
        base_color_texture: None,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShadingError {
    /// A Math operation or Mix blend type this evaluator does not know.
    UnsupportedSetting { node: String, key: String, value: String },
    /// A node type blenkernel accepts but this evaluator does not handle.
    UnsupportedNode(String),
}

impl fmt::Display for ShadingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ShadingError::UnsupportedSetting { node, key, value } => {
                write!(f, "{node}: unsupported {key} \"{value}\"")
            }
            ShadingError::UnsupportedNode(idname) => write!(f, "unsupported node type {idname}"),
        }
    }
}

impl std::error::Error for ShadingError {}

/// A value on any socket, shader sockets included.
#[derive(Clone, Debug)]
enum Output {
    Data(Value),
    Closure(ShadedMaterial),
}

/// Evaluate one material from `bmain`. Missing handle → `None`.
pub fn evaluate_material(bmain: &Main, material: MaterialId) -> Option<Result<ShadedMaterial, ShadingError>> {
    bmain.material(material).ok().map(evaluate)
}

/// Evaluate a material: its node tree if it has one, otherwise its viewport values
/// (like Blender with "Use Nodes" off).
pub fn evaluate(material: &Material) -> Result<ShadedMaterial, ShadingError> {
    match &material.node_tree {
        Some(tree) => evaluate_tree(tree),
        None => {
            let [r, g, b, a] = material.base_color;
            Ok(ShadedMaterial {
                base_color: [r, g, b],
                alpha: a,
                metallic: material.metallic,
                roughness: material.roughness,
                emission: [0.0; 3],
                base_color_texture: None,
            })
        }
    }
}

/// Evaluate a node tree from its active Material Output's Surface input.
pub fn evaluate_tree(tree: &NodeTree) -> Result<ShadedMaterial, ShadingError> {
    let Some(output) = tree.output_node() else {
        return Ok(ShadedMaterial::EMPTY);
    };
    let mut evaluator = Evaluator { tree, cache: HashMap::new() };
    evaluator.closure_input(output, "Surface")
}

struct Evaluator<'a> {
    tree: &'a NodeTree,
    cache: HashMap<(NodeId, String), Output>,
}

impl Evaluator<'_> {
    fn node(&self, id: NodeId) -> &Node {
        self.tree.node(id).expect("links only reference live nodes")
    }

    /// The value arriving at a non-shader input, converted to the input's type.
    fn data_input(&mut self, node: NodeId, identifier: &str) -> Result<Value, ShadingError> {
        let socket = self.node(node).input(identifier).expect("declared socket");
        let socket_type = socket.socket_type;
        let default = socket.default_value;
        let Some(link) = self.tree.input_link(node, identifier) else {
            return Ok(Value::from_socket(default).unwrap_or(Value::Float(0.0)));
        };
        let (from, from_socket) = (link.from_node, link.from_socket.clone());
        Ok(match self.output(from, &from_socket)? {
            Output::Data(value) => value.convert(socket_type),
            // blenkernel refuses shader → data links; treat one as no input.
            Output::Closure(_) => Value::from_socket(default).unwrap_or(Value::Float(0.0)),
        })
    }

    fn float_input(&mut self, node: NodeId, identifier: &str) -> Result<f32, ShadingError> {
        Ok(self.data_input(node, identifier)?.to_float())
    }

    fn color_input(&mut self, node: NodeId, identifier: &str) -> Result<[f32; 4], ShadingError> {
        Ok(self.data_input(node, identifier)?.to_color())
    }

    /// The closure arriving at a shader input. A color or value linked into a shader input
    /// becomes an emission of that color, as in Blender.
    fn closure_input(&mut self, node: NodeId, identifier: &str) -> Result<ShadedMaterial, ShadingError> {
        let Some(link) = self.tree.input_link(node, identifier) else {
            return Ok(ShadedMaterial::EMPTY);
        };
        let (from, from_socket) = (link.from_node, link.from_socket.clone());
        Ok(match self.output(from, &from_socket)? {
            Output::Closure(closure) => closure,
            Output::Data(value) => {
                let [r, g, b, _] = value.to_color();
                ShadedMaterial { emission: [r, g, b], ..ShadedMaterial::EMPTY }
            }
        })
    }

    /// The image of an Image Texture linked straight into `identifier`, if any.
    fn linked_texture(&self, node: NodeId, identifier: &str) -> Option<String> {
        let link = self.tree.input_link(node, identifier)?;
        let from = self.node(link.from_node);
        let image = from.properties.get("image")?;
        (from.idname == "ShaderNodeTexImage" && link.from_socket == "Color" && !image.is_empty())
            .then(|| image.clone())
    }

    fn output(&mut self, node: NodeId, socket: &str) -> Result<Output, ShadingError> {
        let key = (node, socket.to_string());
        if let Some(cached) = self.cache.get(&key) {
            return Ok(cached.clone());
        }
        let result = self.compute(node, socket)?;
        self.cache.insert(key, result.clone());
        Ok(result)
    }

    fn setting(&self, node: NodeId, key: &str) -> String {
        self.node(node).properties.get(key).cloned().unwrap_or_default()
    }

    fn unsupported(&self, node: NodeId, key: &str) -> ShadingError {
        ShadingError::UnsupportedSetting {
            node: self.node(node).name.clone(),
            key: key.to_string(),
            value: self.setting(node, key),
        }
    }

    fn compute(&mut self, node: NodeId, socket: &str) -> Result<Output, ShadingError> {
        let idname = self.node(node).idname.clone();
        Ok(match idname.as_str() {
            "ShaderNodeBsdfPrincipled" => {
                let [r, g, b, _] = self.color_input(node, "Base Color")?;
                let [er, eg, eb, _] = self.color_input(node, "Emission Color")?;
                let strength = self.float_input(node, "Emission Strength")?;
                Output::Closure(ShadedMaterial {
                    base_color: [r, g, b],
                    alpha: self.float_input(node, "Alpha")?.clamp(0.0, 1.0),
                    metallic: self.float_input(node, "Metallic")?.clamp(0.0, 1.0),
                    roughness: self.float_input(node, "Roughness")?.clamp(0.0, 1.0),
                    emission: [er * strength, eg * strength, eb * strength],
                    base_color_texture: self.linked_texture(node, "Base Color"),
                })
            }
            // A Lambert-style diffuse: fully rough, no specular.
            "ShaderNodeBsdfDiffuse" => {
                let [r, g, b, _] = self.color_input(node, "Color")?;
                Output::Closure(ShadedMaterial {
                    base_color: [r, g, b],
                    base_color_texture: self.linked_texture(node, "Color"),
                    ..ShadedMaterial::EMPTY
                })
            }
            "ShaderNodeEmission" => {
                let [r, g, b, _] = self.color_input(node, "Color")?;
                let strength = self.float_input(node, "Strength")?;
                Output::Closure(ShadedMaterial {
                    emission: [r * strength, g * strength, b * strength],
                    ..ShadedMaterial::EMPTY
                })
            }
            "ShaderNodeMixShader" => {
                let fac = self.float_input(node, "Fac")?.clamp(0.0, 1.0);
                let a = self.closure_input(node, "Shader")?;
                let b = self.closure_input(node, "Shader_001")?;
                Output::Closure(mix_closures(fac, a, b))
            }
            "ShaderNodeMix" => {
                let fac = self.float_input(node, "Factor_Float")?.clamp(0.0, 1.0);
                let a = self.color_input(node, "A_Color")?;
                let b = self.color_input(node, "B_Color")?;
                let blend = self.setting(node, "blend_type");
                let mut result =
                    ops::mix_color(&blend, fac, a, b).ok_or_else(|| self.unsupported(node, "blend_type"))?;
                if self.setting(node, "clamp_result") == "true" {
                    for c in &mut result[..3] {
                        *c = c.clamp(0.0, 1.0);
                    }
                }
                Output::Data(Value::Color(result))
            }
            "ShaderNodeMath" => {
                let a = self.float_input(node, "Value")?;
                let b = self.float_input(node, "Value_001")?;
                let c = self.float_input(node, "Value_002")?;
                let operation = self.setting(node, "operation");
                let mut result = ops::math(&operation, a, b, c).ok_or_else(|| self.unsupported(node, "operation"))?;
                if self.setting(node, "use_clamp") == "true" {
                    result = result.clamp(0.0, 1.0);
                }
                Output::Data(Value::Float(result))
            }
            // RGB and Value nodes store their value on the output socket.
            "ShaderNodeRGB" | "ShaderNodeValue" => {
                let default = self.node(node).output(socket).map(|s| s.default_value);
                Output::Data(default.and_then(Value::from_socket).unwrap_or(Value::Float(0.0)))
            }
            // Pixels aren't sampled here: with an image the color is white so the renderer's
            // texture sample passes through unchanged; without one it is Blender's
            // missing-texture magenta.
            "ShaderNodeTexImage" => {
                let has_image = !self.setting(node, "image").is_empty();
                Output::Data(match (socket, has_image) {
                    ("Alpha", _) => Value::Float(1.0),
                    (_, true) => Value::Color([1.0, 1.0, 1.0, 1.0]),
                    (_, false) => Value::Color([1.0, 0.0, 1.0, 1.0]),
                })
            }
            // Only reached when an output node is linked into something; it has no outputs.
            "ShaderNodeOutputMaterial" => Output::Closure(ShadedMaterial::EMPTY),
            _ => return Err(ShadingError::UnsupportedNode(idname)),
        })
    }
}

fn mix_closures(fac: f32, a: ShadedMaterial, b: ShadedMaterial) -> ShadedMaterial {
    let lerp = |x: f32, y: f32| x * (1.0 - fac) + y * fac;
    let lerp3 = |x: [f32; 3], y: [f32; 3]| [lerp(x[0], y[0]), lerp(x[1], y[1]), lerp(x[2], y[2])];
    ShadedMaterial {
        base_color: lerp3(a.base_color, b.base_color),
        alpha: lerp(a.alpha, b.alpha),
        metallic: lerp(a.metallic, b.metallic),
        roughness: lerp(a.roughness, b.roughness),
        emission: lerp3(a.emission, b.emission),
        // One texture per material: keep the one with the larger weight.
        base_color_texture: if fac <= 0.5 {
            a.base_color_texture.or(b.base_color_texture)
        } else {
            b.base_color_texture.or(a.base_color_texture)
        },
    }
}
