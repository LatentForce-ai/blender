//! Shader node-tree evaluation checks (HACK-7).

use blenkernel::{primitives, recalc, Change, IdType, Main, Material, NodeTree, ObjectData, SocketInOut, SocketValue};
use glam::Vec3;
use shading::{evaluate, evaluate_tree, ShadedMaterial, ShadingCache, ShadingError};

const EPS: f32 = 1e-5;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < EPS
}

fn close3(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| close(*x, y))
}

fn set(tree: &mut NodeTree, node: blenkernel::NodeId, socket: &str, value: SocketValue) {
    tree.set_socket_value(node, SocketInOut::Input, socket, value).unwrap();
}

fn set_out(tree: &mut NodeTree, node: blenkernel::NodeId, socket: &str, value: SocketValue) {
    tree.set_socket_value(node, SocketInOut::Output, socket, value).unwrap();
}

/// Principled BSDF → Material Output, returning (tree, principled).
fn principled_tree() -> (NodeTree, blenkernel::NodeId) {
    let mut tree = NodeTree::new();
    let principled = tree.add_node("ShaderNodeBsdfPrincipled").unwrap();
    let output = tree.add_node("ShaderNodeOutputMaterial").unwrap();
    tree.add_link(principled, "BSDF", output, "Surface").unwrap();
    (tree, principled)
}

#[test]
fn default_material_evaluates_to_principled_defaults() {
    let result = evaluate(&Material::with_default_nodes()).unwrap();
    assert_eq!(result.base_color, [0.8, 0.8, 0.8]);
    assert_eq!((result.alpha, result.metallic, result.roughness), (1.0, 0.0, 0.5));
    assert_eq!(result.emission, [0.0; 3]);
    assert_eq!(result.base_color_texture, None);
}

#[test]
fn material_without_nodes_uses_viewport_values() {
    let mut material = Material::new();
    material.base_color = [0.1, 0.2, 0.3, 0.5];
    material.metallic = 0.7;
    let result = evaluate(&material).unwrap();
    assert_eq!(result.base_color, [0.1, 0.2, 0.3]);
    assert_eq!((result.alpha, result.metallic, result.roughness), (0.5, 0.7, 0.4));
}

#[test]
fn principled_inputs_are_read_and_clamped() {
    let (mut tree, p) = principled_tree();
    set(&mut tree, p, "Base Color", SocketValue::Color([1.0, 0.0, 0.0, 1.0]));
    set(&mut tree, p, "Metallic", SocketValue::Float(2.0));
    set(&mut tree, p, "Roughness", SocketValue::Float(0.25));
    set(&mut tree, p, "Alpha", SocketValue::Float(0.5));
    set(&mut tree, p, "Emission Color", SocketValue::Color([1.0, 0.5, 0.0, 1.0]));
    set(&mut tree, p, "Emission Strength", SocketValue::Float(2.0));
    let result = evaluate_tree(&tree).unwrap();
    assert_eq!(result.base_color, [1.0, 0.0, 0.0]);
    assert_eq!((result.alpha, result.metallic, result.roughness), (0.5, 1.0, 0.25));
    assert_eq!(result.emission, [2.0, 1.0, 0.0]);
}

#[test]
fn rgb_through_mix_into_base_color() {
    let (mut tree, p) = principled_tree();
    let red = tree.add_node("ShaderNodeRGB").unwrap();
    let blue = tree.add_node("ShaderNodeRGB").unwrap();
    let mix = tree.add_node("ShaderNodeMix").unwrap();
    set_out(&mut tree, red, "Color", SocketValue::Color([1.0, 0.0, 0.0, 1.0]));
    set_out(&mut tree, blue, "Color", SocketValue::Color([0.0, 0.0, 1.0, 1.0]));
    set(&mut tree, mix, "Factor_Float", SocketValue::Float(0.25));
    tree.add_link(red, "Color", mix, "A_Color").unwrap();
    tree.add_link(blue, "Color", mix, "B_Color").unwrap();
    tree.add_link(mix, "Result_Color", p, "Base Color").unwrap();

    assert!(close3(evaluate_tree(&tree).unwrap().base_color, [0.75, 0.0, 0.25]));

    tree.set_property(mix, "blend_type", "MULTIPLY").unwrap();
    set(&mut tree, mix, "Factor_Float", SocketValue::Float(1.0));
    assert!(close3(evaluate_tree(&tree).unwrap().base_color, [0.0, 0.0, 0.0]));

    tree.set_property(mix, "blend_type", "ADD").unwrap();
    assert!(close3(evaluate_tree(&tree).unwrap().base_color, [1.0, 0.0, 1.0]));

    // Factor is clamped to 0..1.
    tree.set_property(mix, "blend_type", "MIX").unwrap();
    set(&mut tree, mix, "Factor_Float", SocketValue::Float(3.0));
    assert!(close3(evaluate_tree(&tree).unwrap().base_color, [0.0, 0.0, 1.0]));
}

#[test]
fn value_and_math_drive_float_inputs() {
    let (mut tree, p) = principled_tree();
    let value = tree.add_node("ShaderNodeValue").unwrap();
    let math = tree.add_node("ShaderNodeMath").unwrap();
    set_out(&mut tree, value, "Value", SocketValue::Float(0.3));
    set(&mut tree, math, "Value_001", SocketValue::Float(2.0));
    tree.set_property(math, "operation", "MULTIPLY").unwrap();
    tree.add_link(value, "Value", math, "Value").unwrap();
    tree.add_link(math, "Value", p, "Roughness").unwrap();
    assert!(close(evaluate_tree(&tree).unwrap().roughness, 0.6));

    // use_clamp, then the Principled clamp on top of an unclamped result.
    tree.set_property(math, "operation", "ADD").unwrap();
    set(&mut tree, math, "Value_001", SocketValue::Float(5.0));
    assert_eq!(evaluate_tree(&tree).unwrap().roughness, 1.0);
    tree.add_link(math, "Value", p, "Metallic").unwrap();
    tree.set_property(math, "operation", "SUBTRACT").unwrap();
    tree.set_property(math, "use_clamp", "true").unwrap();
    assert_eq!(evaluate_tree(&tree).unwrap().metallic, 0.0);
}

#[test]
fn math_operations_match_blender() {
    use shading::ops::math;
    assert_eq!(math("DIVIDE", 1.0, 0.0, 0.0), Some(0.0));
    assert_eq!(math("POWER", -2.0, 0.5, 0.0), Some(0.0));
    assert_eq!(math("POWER", -2.0, 2.0, 0.0), Some(4.0));
    assert_eq!(math("SQRT", -4.0, 0.0, 0.0), Some(0.0));
    assert_eq!(math("MULTIPLY_ADD", 2.0, 3.0, 1.0), Some(7.0));
    assert_eq!(math("MODULO", -5.0, 3.0, 0.0), Some(-2.0));
    assert_eq!(math("FLOORED_MODULO", -5.0, 3.0, 0.0), Some(1.0));
    assert_eq!(math("WRAP", 5.0, 3.0, 1.0), Some(1.0));
    assert_eq!(math("PINGPONG", 3.0, 2.0, 0.0), Some(1.0));
    assert_eq!(math("ROUND", 2.5, 0.0, 0.0), Some(3.0));
    assert_eq!(math("COMPARE", 1.0, 1.05, 0.1), Some(1.0));
    assert_eq!(math("NOPE", 1.0, 1.0, 1.0), None);
}

#[test]
fn color_into_float_uses_luminance() {
    let (mut tree, p) = principled_tree();
    let rgb = tree.add_node("ShaderNodeRGB").unwrap();
    set_out(&mut tree, rgb, "Color", SocketValue::Color([0.0, 1.0, 0.0, 1.0]));
    tree.add_link(rgb, "Color", p, "Roughness").unwrap();
    assert!(close(evaluate_tree(&tree).unwrap().roughness, 0.7152));
}

#[test]
fn mix_shader_blends_closures() {
    let mut tree = NodeTree::new();
    let diffuse = tree.add_node("ShaderNodeBsdfDiffuse").unwrap();
    let emission = tree.add_node("ShaderNodeEmission").unwrap();
    let mix = tree.add_node("ShaderNodeMixShader").unwrap();
    let output = tree.add_node("ShaderNodeOutputMaterial").unwrap();
    set(&mut tree, diffuse, "Color", SocketValue::Color([1.0, 0.0, 0.0, 1.0]));
    set(&mut tree, emission, "Color", SocketValue::Color([0.0, 1.0, 0.0, 1.0]));
    set(&mut tree, emission, "Strength", SocketValue::Float(4.0));
    set(&mut tree, mix, "Fac", SocketValue::Float(0.25));
    tree.add_link(diffuse, "BSDF", mix, "Shader").unwrap();
    tree.add_link(emission, "Emission", mix, "Shader_001").unwrap();
    tree.add_link(mix, "Shader", output, "Surface").unwrap();

    let result = evaluate_tree(&tree).unwrap();
    assert!(close3(result.base_color, [0.75, 0.0, 0.0]));
    assert!(close3(result.emission, [0.0, 1.0, 0.0]));
    assert!(close(result.roughness, 1.0));

    // An empty shader input mixes in black.
    tree.remove_link(mix, "Shader_001");
    assert!(close3(evaluate_tree(&tree).unwrap().emission, [0.0; 3]));
}

#[test]
fn color_linked_into_shader_input_is_emission() {
    let mut tree = NodeTree::new();
    let rgb = tree.add_node("ShaderNodeRGB").unwrap();
    let output = tree.add_node("ShaderNodeOutputMaterial").unwrap();
    set_out(&mut tree, rgb, "Color", SocketValue::Color([0.2, 0.4, 0.6, 1.0]));
    tree.add_link(rgb, "Color", output, "Surface").unwrap();
    let result = evaluate_tree(&tree).unwrap();
    assert_eq!(result.emission, [0.2, 0.4, 0.6]);
    assert_eq!(result.base_color, [0.0; 3]);
}

#[test]
fn missing_surface_or_output_draws_black() {
    let mut tree = NodeTree::new();
    tree.add_node("ShaderNodeBsdfPrincipled").unwrap();
    assert_eq!(evaluate_tree(&tree).unwrap().base_color, [0.0; 3]);
    tree.add_node("ShaderNodeOutputMaterial").unwrap();
    assert_eq!(evaluate_tree(&tree).unwrap().base_color, [0.0; 3]);
}

#[test]
fn image_texture_is_reported_for_the_renderer() {
    let (mut tree, p) = principled_tree();
    let tex = tree.add_node("ShaderNodeTexImage").unwrap();
    tree.add_link(tex, "Color", p, "Base Color").unwrap();

    // No image: Blender's missing-texture magenta, no texture to sample.
    let result = evaluate_tree(&tree).unwrap();
    assert_eq!(result.base_color, [1.0, 0.0, 1.0]);
    assert_eq!(result.base_color_texture, None);

    tree.set_property(tex, "image", "bricks.png").unwrap();
    let result = evaluate_tree(&tree).unwrap();
    assert_eq!(result.base_color, [1.0, 1.0, 1.0]);
    assert_eq!(result.base_color_texture.as_deref(), Some("bricks.png"));
}

#[test]
fn unknown_settings_are_errors() {
    let (mut tree, p) = principled_tree();
    let math = tree.add_node("ShaderNodeMath").unwrap();
    tree.set_property(math, "operation", "TELEPORT").unwrap();
    tree.add_link(math, "Value", p, "Roughness").unwrap();
    assert!(matches!(evaluate_tree(&tree), Err(ShadingError::UnsupportedSetting { .. })));
}

#[test]
fn shared_upstream_node_is_evaluated_once_per_tree() {
    // One RGB feeding two inputs through a Mix: both read the same cached output.
    let (mut tree, p) = principled_tree();
    let rgb = tree.add_node("ShaderNodeRGB").unwrap();
    set_out(&mut tree, rgb, "Color", SocketValue::Color([0.5, 0.5, 0.5, 1.0]));
    tree.add_link(rgb, "Color", p, "Base Color").unwrap();
    tree.add_link(rgb, "Color", p, "Emission Color").unwrap();
    set(&mut tree, p, "Emission Strength", SocketValue::Float(1.0));
    let result = evaluate_tree(&tree).unwrap();
    assert_eq!(result.base_color, [0.5; 3]);
    assert_eq!(result.emission, [0.5; 3]);
}

#[test]
fn cache_follows_main_changes() {
    let mut bmain = Main::new();
    let red = bmain.material_add("Red", true);
    let plain = bmain.material_add("Plain", false);
    let mesh = bmain.mesh_add("Cube", primitives::cube(Vec3::splat(2.0)));
    bmain.mesh_set_materials(mesh, vec![Some(red), None]).unwrap();
    let object = bmain.object_add("Cube", ObjectData::Mesh(mesh)).unwrap();

    let mut cache = ShadingCache::new();
    cache.rebuild(&bmain);
    bmain.take_changes();
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.object_slot(&bmain, object, 0).base_color, [0.8; 3]);
    // Empty slot → default surface.
    assert_eq!(cache.object_slot(&bmain, object, 1), &ShadedMaterial::DEFAULT_SURFACE);

    // Edit the Principled base color: only that material is reported.
    {
        let tree = bmain.material_mut(red).unwrap().node_tree.as_mut().unwrap();
        let p = tree.node_by_name("Principled BSDF").unwrap();
        set(tree, p, "Base Color", SocketValue::Color([1.0, 0.0, 0.0, 1.0]));
    }
    let changes = bmain.take_changes();
    assert!(changes.iter().any(|c| c.id_type == IdType::Material && c.flags & recalc::SHADING != 0));
    assert_eq!(cache.apply_changes(&bmain, &changes), vec![red]);
    assert_eq!(cache.get(red).base_color, [1.0, 0.0, 0.0]);

    // A SHADING tag without a real change is not re-uploaded.
    let noop = [Change { id_type: IdType::Material, handle: red.0, flags: recalc::SHADING }];
    assert!(cache.apply_changes(&bmain, &noop).is_empty());

    // Removed materials are dropped.
    bmain.material_remove(plain).unwrap();
    cache.apply_changes(&bmain, &[]);
    assert_eq!(cache.len(), 1);
}

#[test]
fn refresh_works_without_change_flags() {
    // The depsgraph drains take_changes(); refresh() must still see material edits.
    let mut bmain = Main::new();
    let a = bmain.material_add("A", true);
    let b = bmain.material_add("B", true);
    let mut cache = ShadingCache::new();
    assert_eq!(cache.refresh(&bmain).len(), 2);
    assert!(cache.refresh(&bmain).is_empty());

    {
        let tree = bmain.material_mut(b).unwrap().node_tree.as_mut().unwrap();
        let p = tree.node_by_name("Principled BSDF").unwrap();
        set(tree, p, "Roughness", SocketValue::Float(0.9));
    }
    bmain.take_changes(); // as if the depsgraph took them first
    assert_eq!(cache.refresh(&bmain), vec![b]);
    assert_eq!(cache.get(b).roughness, 0.9);
    assert_eq!(cache.get(a).roughness, 0.5);
}

#[test]
fn results_serialize_for_the_js_side() {
    let json = serde_json::to_value(evaluate(&Material::with_default_nodes()).unwrap()).unwrap();
    assert_eq!(json["roughness"], 0.5);
    assert!(json["base_color_texture"].is_null());
}
