//! Material and shader node tree checks (HACK-9).

use blenkernel::node::SocketInOut;
use blenkernel::node_types::{declaration, NODE_TYPES};
use blenkernel::{
    primitives, recalc, BkeError, Change, IdType, Main, Material, NodeTree, ObjectData, SocketValue,
};
use glam::Vec3;

#[test]
fn material_defaults_match_dna() {
    let material = Material::new();
    assert_eq!(material.base_color, [0.8, 0.8, 0.8, 1.0]);
    assert_eq!(material.roughness, 0.4);
    assert_eq!(material.metallic, 0.0);
    assert!(material.node_tree.is_none());
}

#[test]
fn default_surface_tree_matches_blender() {
    let material = Material::with_default_nodes();
    let tree = material.node_tree.as_ref().unwrap();

    let principled = tree.node_by_name("Principled BSDF").unwrap();
    let output = tree.node_by_name("Material Output").unwrap();
    assert_eq!(tree.node(principled).unwrap().idname, "ShaderNodeBsdfPrincipled");
    assert_eq!(tree.output_node(), Some(output));

    let link = tree.input_link(output, "Surface").unwrap();
    assert_eq!((link.from_node, link.from_socket.as_str()), (principled, "BSDF"));

    let node = tree.node(principled).unwrap();
    assert_eq!(node.input("Base Color").unwrap().default_value, SocketValue::Color([0.8, 0.8, 0.8, 1.0]));
    assert_eq!(node.input("Roughness").unwrap().default_value, SocketValue::Float(0.5));
    assert_eq!(node.input("IOR").unwrap().default_value, SocketValue::Float(1.5));
    assert_eq!(node.location, [-200.0, 100.0]);
}

#[test]
fn every_registered_type_has_a_declaration() {
    for idname in NODE_TYPES {
        let decl = declaration(idname).unwrap_or_else(|| panic!("{idname} missing"));
        let mut ids: Vec<_> = decl.inputs.iter().map(|s| s.identifier).collect();
        ids.dedup();
        assert_eq!(ids.len(), decl.inputs.len(), "{idname}: duplicate input identifiers");
    }
    assert!(declaration("ShaderNodeDoesNotExist").is_none());
}

#[test]
fn node_names_are_unique() {
    let mut tree = NodeTree::new();
    let a = tree.add_node("ShaderNodeMath").unwrap();
    let b = tree.add_node("ShaderNodeMath").unwrap();
    assert_eq!(tree.node(a).unwrap().name, "Math");
    assert_eq!(tree.node(b).unwrap().name, "Math.001");
    assert!(matches!(tree.add_node("Nope"), Err(BkeError::UnknownNodeType(_))));
}

#[test]
fn socket_values_are_type_checked() {
    let mut tree = NodeTree::new();
    let rgb = tree.add_node("ShaderNodeRGB").unwrap();
    let principled = tree.add_node("ShaderNodeBsdfPrincipled").unwrap();

    // RGB nodes store their color on the output socket, as in Blender.
    tree.set_socket_value(rgb, SocketInOut::Output, "Color", SocketValue::Color([1.0, 0.0, 0.0, 1.0]))
        .unwrap();
    assert_eq!(
        tree.node(rgb).unwrap().output("Color").unwrap().default_value,
        SocketValue::Color([1.0, 0.0, 0.0, 1.0])
    );

    assert!(matches!(
        tree.set_socket_value(principled, SocketInOut::Input, "Metallic", SocketValue::Color([0.0; 4])),
        Err(BkeError::SocketTypeMismatch(_))
    ));
    assert!(matches!(
        tree.set_socket_value(principled, SocketInOut::Input, "Nope", SocketValue::Float(1.0)),
        Err(BkeError::UnknownSocket(_))
    ));
    assert!(matches!(
        tree.set_socket_value(principled, SocketInOut::Output, "BSDF", SocketValue::None),
        Err(BkeError::SocketTypeMismatch(_))
    ));
}

#[test]
fn links_are_validated() {
    let mut tree = NodeTree::new();
    let rgb = tree.add_node("ShaderNodeRGB").unwrap();
    let mix = tree.add_node("ShaderNodeMix").unwrap();
    let principled = tree.add_node("ShaderNodeBsdfPrincipled").unwrap();
    let output = tree.add_node("ShaderNodeOutputMaterial").unwrap();

    tree.add_link(rgb, "Color", mix, "A_Color").unwrap();
    tree.add_link(mix, "Result_Color", principled, "Base Color").unwrap();
    tree.add_link(principled, "BSDF", output, "Surface").unwrap();

    // Shader into a color input is not allowed.
    assert!(matches!(
        tree.add_link(principled, "BSDF", mix, "B_Color"),
        Err(BkeError::SocketTypeMismatch(_))
    ));
    // Inputs must be inputs, outputs must be outputs.
    assert!(matches!(tree.add_link(mix, "A_Color", principled, "Base Color"), Err(BkeError::UnknownSocket(_))));

    // Replacing the link into an input keeps one link there.
    let rgb2 = tree.add_node("ShaderNodeRGB").unwrap();
    tree.add_link(rgb2, "Color", mix, "A_Color").unwrap();
    assert_eq!(tree.links().iter().filter(|l| l.to_node == mix && l.to_socket == "A_Color").count(), 1);
    assert_eq!(tree.input_link(mix, "A_Color").unwrap().from_node, rgb2);

    // Evaluation order: every node after its inputs.
    let order = tree.topological_order();
    let pos = |n| order.iter().position(|&x| x == n).unwrap();
    assert!(pos(rgb2) < pos(mix) && pos(mix) < pos(principled) && pos(principled) < pos(output));
    assert_eq!(order.len(), 5);

    // Removing a node removes its links.
    tree.remove_node(mix).unwrap();
    assert!(tree.input_link(principled, "Base Color").is_none());
    assert!(matches!(tree.node(mix), Err(BkeError::InvalidNode(_))));
}

#[test]
fn cycles_are_rejected() {
    let mut tree = NodeTree::new();
    let a = tree.add_node("ShaderNodeMath").unwrap();
    let b = tree.add_node("ShaderNodeMath").unwrap();
    let c = tree.add_node("ShaderNodeMath").unwrap();
    tree.add_link(a, "Value", b, "Value").unwrap();
    tree.add_link(b, "Value", c, "Value").unwrap();
    assert_eq!(tree.add_link(c, "Value", a, "Value_001"), Err(BkeError::NodeCycle));
    assert_eq!(tree.add_link(a, "Value", a, "Value_001"), Err(BkeError::NodeCycle));
}

#[test]
fn properties() {
    let mut tree = NodeTree::new();
    let math = tree.add_node("ShaderNodeMath").unwrap();
    assert_eq!(tree.node(math).unwrap().properties["operation"], "ADD");
    tree.set_property(math, "operation", "MULTIPLY").unwrap();
    assert_eq!(tree.node(math).unwrap().properties["operation"], "MULTIPLY");
    assert!(matches!(tree.set_property(math, "nope", "x"), Err(BkeError::UnknownProperty(_))));
}

#[test]
fn active_output_follows_removal() {
    let mut tree = NodeTree::new();
    let first = tree.add_node("ShaderNodeOutputMaterial").unwrap();
    let second = tree.add_node("ShaderNodeOutputMaterial").unwrap();
    assert_eq!(tree.output_node(), Some(first));
    tree.remove_node(first).unwrap();
    assert_eq!(tree.output_node(), Some(second));
}

#[test]
fn json_export() {
    let tree = Material::with_default_nodes().node_tree.unwrap();
    let json: serde_json::Value = serde_json::from_str(&tree.to_json()).unwrap();
    assert_eq!(json["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(json["nodes"][0]["idname"], "ShaderNodeBsdfPrincipled");
    assert_eq!(json["nodes"][0]["inputs"][0]["identifier"], "Base Color");
    assert_eq!(json["nodes"][0]["inputs"][0]["type"], "Color");
    assert_eq!(json["links"][0]["to_socket"], "Surface");
    assert_eq!(json["output_node"], 1);
}

#[test]
fn materials_in_main() {
    let mut bmain = Main::new();
    let red = bmain.material_add("Red", true);
    let blue = bmain.material_add("Material", false);
    let again = bmain.material_add("Red", false);
    assert_eq!(bmain.material(again).unwrap().id.name, "Red.001");
    assert!(bmain.material(red).unwrap().node_tree.is_some());
    assert!(bmain.material(blue).unwrap().node_tree.is_none());

    let mesh = bmain.mesh_add("Cube", primitives::cube(Vec3::splat(2.0)));
    let cube = bmain.object_add("Cube", ObjectData::Mesh(mesh)).unwrap();
    bmain.take_changes();

    bmain.mesh_set_materials(mesh, vec![Some(red), None, Some(blue)]).unwrap();
    assert_eq!(bmain.material(red).unwrap().id.users, 1);
    assert_eq!(
        bmain.take_changes(),
        vec![Change { id_type: IdType::Mesh, handle: mesh.0, flags: recalc::SHADING | recalc::RELATIONS }]
    );

    // Slot lookup through the object, clamped like BKE_object_material_get.
    assert_eq!(bmain.object_material(cube, 0).unwrap(), Some(red));
    assert_eq!(bmain.object_material(cube, 1).unwrap(), None);
    assert_eq!(bmain.object_material(cube, 99).unwrap(), Some(blue));

    // Materials in use can't be removed; replacing slots releases them.
    assert!(matches!(bmain.material_remove(red), Err(BkeError::StillInUse { .. })));
    bmain.mesh_set_materials(mesh, vec![Some(blue)]).unwrap();
    assert_eq!(bmain.material(red).unwrap().id.users, 0);
    bmain.material_remove(red).unwrap();

    // Removing the mesh releases its slots.
    bmain.object_remove(cube).unwrap();
    bmain.mesh_remove(mesh).unwrap();
    assert_eq!(bmain.material(blue).unwrap().id.users, 0);

    // Editing a material reports SHADING.
    bmain.take_changes();
    bmain.material_mut(blue).unwrap().roughness = 0.1;
    assert_eq!(
        bmain.take_changes(),
        vec![Change { id_type: IdType::Material, handle: blue.0, flags: recalc::SHADING }]
    );
    assert!(bmain.mesh_set_materials(mesh, vec![]).is_err(), "removed mesh");
}

#[test]
fn per_face_material_indices() {
    let mut bmain = Main::new();
    let a = bmain.material_add("A", false);
    let b = bmain.material_add("B", false);
    let mesh = bmain.mesh_add("Cube", primitives::cube(Vec3::ONE));
    bmain.mesh_set_materials(mesh, vec![Some(a), Some(b)]).unwrap();

    let cube = bmain.mesh_mut(mesh).unwrap();
    assert_eq!(cube.material_indices(), vec![0; 6]);
    cube.set_material_indices(vec![0, 1, 0, 1, 0, 5]).unwrap();
    assert_eq!(cube.material_indices(), vec![0, 1, 0, 1, 0, 5]);
    // Two triangles per quad; slot 5 clamps to the last slot (1).
    assert_eq!(cube.tri_material_indices(), vec![0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1]);

    assert!(matches!(cube.set_material_indices(vec![0; 3]), Err(BkeError::LengthMismatch { .. })));
    assert!(matches!(cube.set_material_indices(vec![-1; 6]), Err(BkeError::InvalidValue(_))));
}

#[test]
fn images_are_data_blocks_found_by_name() {
    use blenkernel::{Image, ImageId};
    let mut bmain = Main::new();
    let pixels = vec![255u8; 2 * 2 * 4];
    let a = bmain.image_add("bricks.png", Image::new(2, 2, pixels.clone()).unwrap());
    let b = bmain.image_add("bricks.png", Image::new(2, 2, pixels).unwrap());
    assert_eq!(bmain.image(b).unwrap().id.name, "bricks.png.001");
    assert_eq!(bmain.image_by_name("bricks.png"), Some(a));
    assert_eq!(bmain.image_by_name("missing.png"), None);
    assert_eq!(bmain.image(a).unwrap().pixels().len(), 16);

    // New images report a change, like every new data-block.
    assert!(bmain.take_changes().iter().any(|c| c.id_type == IdType::Image && c.handle == a.0));

    assert!(matches!(Image::new(2, 2, vec![0; 3]), Err(BkeError::LengthMismatch { .. })));
    assert!(matches!(Image::new(0, 2, vec![]), Err(BkeError::InvalidValue(_))));

    bmain.image_remove(a).unwrap();
    assert_eq!(bmain.image_by_name("bricks.png"), None);
    assert!(bmain.image(ImageId(a.0)).is_err());
    assert_eq!(bmain.images().count(), 1);
}

#[test]
fn primitives_have_a_uv_map_like_blender() {
    for mesh in [
        primitives::cube(Vec3::splat(2.0)),
        primitives::grid(3, 4, 2.0, 2.0),
        primitives::uv_sphere(1.0, 32, 16),
    ] {
        let uvs = mesh.uv_map().expect("UVMap");
        assert_eq!(uvs.len(), mesh.corners_num());
        assert!(uvs.iter().flatten().all(|c| (0.0..=1.0).contains(c)));
    }
    // Grid corners span the full square; the sphere's top pole is at v = 1.
    let grid = primitives::grid(3, 3, 2.0, 2.0);
    let uvs = grid.uv_map().unwrap();
    assert!(uvs.contains(&[0.0, 0.0]) && uvs.contains(&[1.0, 1.0]));
    let sphere = primitives::uv_sphere(1.0, 8, 4);
    assert_eq!(sphere.uv_map().unwrap()[0], [0.5 / 8.0, 1.0]);
}
