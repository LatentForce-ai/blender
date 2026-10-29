//! Studio lighting for Solid mode: Blender's built-in "Default" studio light
//! (`BKE_studiolight_default` in `blenkernel/intern/studiolight.cc`).

use glam::Vec3;

/// One of a studio light's four directional lights (`SolidLight` in `DNA_userdef_types.h`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidLight {
    /// Unit vector toward the light, in view space.
    pub direction: Vec3,
    pub diffuse: Vec3,
    pub specular: Vec3,
    /// Wrap factor in [0, 1]: larger values soften the light like a bigger source.
    pub smooth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StudioLight {
    pub lights: [SolidLight; 4],
    pub ambient: Vec3,
    /// `STUDIOLIGHT_SPECULAR_HIGHLIGHT_PASS`.
    pub specular_highlights: bool,
}

impl StudioLight {
    /// The studio light Solid mode uses in a factory setup.
    pub const DEFAULT: StudioLight = StudioLight {
        lights: [
            SolidLight {
                direction: Vec3::new(-0.352546, 0.170931, -0.920051),
                diffuse: Vec3::splat(0.033103),
                specular: Vec3::splat(0.266761),
                smooth: 0.526620,
            },
            SolidLight {
                direction: Vec3::new(-0.408163, 0.346939, 0.844415),
                diffuse: Vec3::new(0.521083, 0.538226, 0.538226),
                specular: Vec3::splat(0.599030),
                smooth: 0.0,
            },
            SolidLight {
                direction: Vec3::new(0.521739, 0.826087, 0.212999),
                diffuse: Vec3::new(0.038403, 0.034357, 0.049530),
                specular: Vec3::new(0.106102, 0.125981, 0.158523),
                smooth: 0.478261,
            },
            SolidLight {
                direction: Vec3::new(0.624519, -0.562067, -0.542269),
                diffuse: Vec3::new(0.090838, 0.082080, 0.072255),
                specular: Vec3::new(0.106535, 0.084771, 0.066080),
                smooth: 0.2,
            },
        ],
        ambient: Vec3::ZERO,
        specular_highlights: true,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_light_directions_are_unit_length() {
        for light in StudioLight::DEFAULT.lights {
            assert!((light.direction.length() - 1.0).abs() < 1e-4);
        }
    }
}
