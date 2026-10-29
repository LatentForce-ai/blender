//! The material values Solid mode draws (`workbench::Material`): a material's viewport display
//! values, not its node tree. Empty slots and objects without materials use the default.

use blenkernel::Material;

/// Viewport display values of one material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidMaterial {
    pub base_color: [f32; 3],
    pub alpha: f32,
    pub metallic: f32,
    pub roughness: f32,
}

impl Default for SolidMaterial {
    /// `Material` defaults from `makesdna/DNA_material_types.h`.
    fn default() -> Self {
        SolidMaterial {
            base_color: [0.8; 3],
            alpha: 1.0,
            metallic: 0.0,
            roughness: 0.4,
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
        }
    }
}

impl SolidMaterial {
    /// Roughness remapped the way workbench feeds its lighting (`Material::pack_data`).
    pub fn disney_roughness(&self) -> f32 {
        self.roughness.sqrt()
    }
}
