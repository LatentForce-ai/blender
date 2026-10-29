//! Shader node-tree evaluation for the Blender web migration (HACK-7).
//!
//! blenkernel stores materials and their node trees; this crate turns each material into the
//! flat values the render pipeline draws with ([`ShadedMaterial`]): Principled-style base color,
//! alpha, metallic, roughness and emission, plus the image a texture node feeds into the base
//! color. It is a constant-folding evaluator — the counterpart of Blender's GPU material code
//! generation (`gpu/intern/gpu_material.cc`, `nodes/shader/node_shader_tree.cc`) for the subset
//! of node types blenkernel supports, without per-pixel inputs such as UVs.

pub mod cache;
pub mod eval;
pub mod ops;
pub mod value;

pub use cache::ShadingCache;
pub use eval::{evaluate, evaluate_material, evaluate_tree, ShadedMaterial, ShadingError};
