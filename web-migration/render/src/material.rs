//! The material values the viewport draws. Solid mode (`workbench::Material`) uses a material's
//! viewport display values, not its node tree; Material Preview uses the node tree as evaluated
//! by the shading crate (HACK-7). Empty slots and objects without materials use the default.

use blenkernel::{ImageId, Material};
use shading::ShadedMaterial;

/// `View3DShading::type`: how the viewport colours surfaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadingType {
    /// `OB_SOLID`: viewport display values with studio lighting.
    #[default]
    Solid,
    /// `OB_MATERIAL`: node-tree results. Lit with the same studio light rather than EEVEE with
    /// an HDRI.
    Material,
}

impl ShadingType {
    /// From Blender's RNA identifier (`"SOLID"`, `"MATERIAL"`).
    pub fn from_identifier(identifier: &str) -> Option<ShadingType> {
        match identifier {
            "SOLID" => Some(ShadingType::Solid),
            "MATERIAL" => Some(ShadingType::Material),
            _ => None,
        }
    }

    pub fn identifier(self) -> &'static str {
        match self {
            ShadingType::Solid => "SOLID",
            ShadingType::Material => "MATERIAL",
        }
    }
}

/// The values one material slot is drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidMaterial {
    pub base_color: [f32; 3],
    pub alpha: f32,
    pub metallic: f32,
    pub roughness: f32,
    /// Added on top of lighting. Always black in Solid mode.
    pub emission: [f32; 3],
    /// Image multiplied into the base colour in Material Preview (resolved by the viewport from
    /// the Image Texture node's image name). Solid mode never textures, like Blender's
    /// Material colour type.
    pub texture: Option<ImageId>,
}

impl Default for SolidMaterial {
    /// `Material` defaults from `makesdna/DNA_material_types.h`.
    fn default() -> Self {
        SolidMaterial {
            base_color: [0.8; 3],
            alpha: 1.0,
            metallic: 0.0,
            roughness: 0.4,
            emission: [0.0; 3],
            texture: None,
        }
    }
}

impl From<&Material> for SolidMaterial {
    /// `workbench::Material(::Material &mat)`.
    fn from(material: &Material) -> Self {
        let [r, g, b, alpha] = material.base_color;
        SolidMaterial {
            base_color: [r, g, b],
            alpha,
            metallic: material.metallic,
            roughness: material.roughness,
            emission: [0.0; 3],
            texture: None,
        }
    }
}

impl From<&ShadedMaterial> for SolidMaterial {
    /// A node-tree result for Material Preview. `texture` is left for the viewport to resolve,
    /// since the result only names the image.
    fn from(material: &ShadedMaterial) -> Self {
        SolidMaterial {
            base_color: material.base_color,
            alpha: material.alpha,
            metallic: material.metallic,
            roughness: material.roughness,
            emission: material.emission,
            texture: None,
        }
    }
}

impl SolidMaterial {
    /// Roughness remapped the way workbench feeds its lighting (`Material::pack_data`).
    pub fn disney_roughness(&self) -> f32 {
        self.roughness.sqrt()
    }

    /// Alpha below 1 is drawn blended after the opaque surfaces, like workbench's transparent
    /// pipeline in Solid mode and EEVEE's "Blended" render method in Material Preview.
    pub fn is_transparent(&self) -> bool {
        self.alpha < 1.0
    }
}

/// Orders transparent draws far to near by their view-space depth (`-z` is in front of the
/// camera, so the most negative `z` is drawn first), as blended drawing needs.
pub fn sort_back_to_front<T>(draws: &mut [(f32, T)]) {
    draws.sort_by(|a, b| a.0.total_cmp(&b.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shading_type_round_trips_blender_identifiers() {
        for shading_type in [ShadingType::Solid, ShadingType::Material] {
            assert_eq!(ShadingType::from_identifier(shading_type.identifier()), Some(shading_type));
        }
        assert_eq!(ShadingType::from_identifier("RENDERED"), None);
        assert_eq!(ShadingType::default(), ShadingType::Solid);
    }

    #[test]
    fn transparency_follows_alpha_in_both_modes() {
        let mut material = Material::with_default_nodes();
        assert!(!SolidMaterial::from(&material).is_transparent());
        material.base_color[3] = 0.5;
        assert!(SolidMaterial::from(&material).is_transparent());

        let tree = material.node_tree.as_mut().unwrap();
        let principled = tree.node_by_name("Principled BSDF").unwrap();
        let preview = SolidMaterial::from(&shading::evaluate(&material).unwrap());
        assert!(!preview.is_transparent(), "Material Preview reads the node's Alpha, not the viewport one");
        let tree = material.node_tree.as_mut().unwrap();
        tree.set_socket_value(principled, blenkernel::SocketInOut::Input, "Alpha", blenkernel::SocketValue::Float(0.3))
            .unwrap();
        let preview = SolidMaterial::from(&shading::evaluate(&material).unwrap());
        assert!(preview.is_transparent());
        assert_eq!(preview.alpha, 0.3);
    }

    #[test]
    fn transparent_draws_go_far_to_near() {
        let mut draws = vec![(-2.0, "near"), (-10.0, "far"), (-5.0, "middle")];
        sort_back_to_front(&mut draws);
        assert_eq!(draws.iter().map(|d| d.1).collect::<Vec<_>>(), ["far", "middle", "near"]);
    }

    #[test]
    fn material_preview_uses_node_results_and_solid_ignores_them() {
        // A red Principled BSDF with emission, while the viewport display color stays grey.
        let mut material = Material::with_default_nodes();
        let tree = material.node_tree.as_mut().unwrap();
        let principled = tree.node_by_name("Principled BSDF").unwrap();
        let input = blenkernel::SocketInOut::Input;
        tree.set_socket_value(principled, input, "Base Color", blenkernel::SocketValue::Color([1.0, 0.0, 0.0, 1.0]))
            .unwrap();
        tree.set_socket_value(principled, input, "Emission Strength", blenkernel::SocketValue::Float(2.0))
            .unwrap();

        let solid = SolidMaterial::from(&material);
        assert_eq!((solid.base_color, solid.emission), ([0.8; 3], [0.0; 3]));

        let preview = SolidMaterial::from(&shading::evaluate(&material).unwrap());
        assert_eq!(preview.base_color, [1.0, 0.0, 0.0]);
        assert_eq!(preview.emission, [2.0; 3]);
        assert_eq!((preview.metallic, preview.roughness), (0.0, 0.5));
    }
}
