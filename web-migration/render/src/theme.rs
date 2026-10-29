//! Default theme colours the viewport draws with
//! (`release/datafiles/userdef/userdef_default_theme.c`), as 8-bit display sRGB with alpha.

pub type ThemeColor = [u8; 4];

/// `space_view3d.back`.
pub const VIEW3D_BACKGROUND: ThemeColor = [0x3d, 0x3d, 0x3d, 0xff];
/// `space_view3d.grid`.
pub const VIEW3D_GRID: ThemeColor = [0x54, 0x54, 0x54, 0x80];
/// `space_view3d.grid_major`.
pub const VIEW3D_GRID_MAJOR: ThemeColor = [0x54, 0x54, 0x54, 0xff];
/// `space_view3d.grid_axis_brightness`.
pub const VIEW3D_GRID_AXIS_BRIGHTNESS: f32 = 0.46;
/// `tui.xaxis`, `tui.yaxis`, `tui.zaxis`.
pub const AXES: [ThemeColor; 3] = [
    [0xff, 0x33, 0x52, 0xff],
    [0x8b, 0xdc, 0x00, 0xff],
    [0x28, 0x90, 0xff, 0xff],
];

/// Display sRGB to scene-linear, for one channel in [0, 1].
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Theme colour as floats, still display sRGB (`rgba_uchar_to_float`).
pub fn to_float(color: ThemeColor) -> [f32; 4] {
    color.map(|channel| f32::from(channel) / 255.0)
}

/// Converts the colour channels to scene-linear; alpha is unchanged.
pub fn to_linear([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a]
}

/// Brightens or darkens by `offset` on the 0-255 scale, alpha untouched
/// (`ui::theme::get_color_shade_4fv`).
pub fn shade(color: ThemeColor, offset: i32) -> [f32; 4] {
    let [r, g, b, a] = color.map(i32::from);
    let shift = |channel: i32| (channel + offset).clamp(0, 255) as f32 / 255.0;
    [shift(r), shift(g), shift(b), a as f32 / 255.0]
}

/// Mixes two colours by `fac`, then shades the colour channels by `offset`
/// (`ui::theme::get_color_blend_shade_4fv`).
pub fn blend_shade(from: ThemeColor, to: ThemeColor, fac: f32, offset: i32) -> [f32; 4] {
    let fac = fac.clamp(0.0, 1.0);
    let mix = |i: usize| ((1.0 - fac) * f32::from(from[i]) + fac * f32::from(to[i])).floor() as i32;
    let shift = |i: usize| (offset + mix(i)).clamp(0, 255) as f32 / 255.0;
    [
        shift(0),
        shift(1),
        shift(2),
        mix(3).clamp(0, 255) as f32 / 255.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_clamps_and_keeps_alpha() {
        assert_eq!(
            shade([250, 5, 100, 128], 10),
            [1.0, 15.0 / 255.0, 110.0 / 255.0, 128.0 / 255.0]
        );
        assert_eq!(shade([250, 5, 100, 128], -10)[1], 0.0);
    }

    #[test]
    fn srgb_to_linear_matches_known_values() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-5);
    }
}
