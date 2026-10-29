//! CPU side of the 3D viewport floor grid: a port of `init_v3d` in
//! `draw/engines/overlay/overlay_grid.hh` for a perspective free view with Blender's default
//! overlays (floor grid, X and Y axes), and the grid colours from `overlay_instance.cc`.

use std::array;

use glam::{Vec2, Vec3};

use crate::theme;

/// `OVERLAY_GRID_STEPS_LEN`: grid levels available.
pub const STEPS_LEN: usize = 8;
/// `OVERLAY_GRID_STEPS_DRAW`: levels drawn at once.
pub const STEPS_DRAW: u32 = 3;
/// `OVERLAY_GRID_ITER_LEN`: depth-biased repeats in perspective, fading the grid through geometry.
pub const ITER_LEN: u32 = 4;
/// Two vertices per axis line, three axes.
pub const AXIS_VERTEX_COUNT: u32 = 6;
/// Lines per level in perspective views.
const PERSPECTIVE_NUM_LINES: u32 = 151;
/// Grid steps grow tenfold per level.
const STEP_GROWTH: f32 = 10.0;

/// `OVERLAY_GridBits`.
pub mod flags {
    pub const SHOW_GRID: u32 = 1 << 0;
    pub const SHOW_AXES: u32 = 1 << 1;
    pub const AXIS_X: u32 = 1 << 2;
    pub const AXIS_Y: u32 = 1 << 3;
    pub const PLANE_XY: u32 = 1 << 5;
}

/// What the grid shaders need per frame (`OVERLAY_GridData` plus the draw flags).
#[derive(Clone, Debug, PartialEq)]
pub struct GridParams {
    /// Line spacing per level, in metres.
    pub steps: [f32; STEPS_LEN],
    /// Floor point the grid is centred on.
    pub offset: Vec2,
    /// Half-extent beyond which lines are clipped.
    pub clip: f32,
    /// Fractional level for the current camera distance.
    pub level: f32,
    pub num_lines: u32,
    pub grid_flag: u32,
    pub axis_flag: u32,
}

impl GridParams {
    /// Vertices per draw: the axes, then two lines (one per direction) per level and index.
    pub fn vertex_count(&self) -> u32 {
        AXIS_VERTEX_COUNT + 4 * STEPS_DRAW * self.num_lines
    }
}

/// Grid parameters for a perspective view at `view_position`, whose view-space +Z axis in world
/// space is `view_backward` (`viewinv[2]`).
pub fn perspective_grid(view_position: Vec3, view_backward: Vec3, clip_end: f32) -> GridParams {
    let steps = metric_steps();
    // Distance to a floor point of interest: between the point looked at and the point below
    // the camera, by azimuth. `safe_divide` keeps an exactly horizontal view finite.
    let along_view = safe_divide(view_position.z, view_backward.z).abs();
    let t = 1.0 - view_backward.z.abs();
    let dist = along_view * (1.0 - t) + view_position.z.abs() * t;
    let offset = (view_position - dist * view_backward).truncate();
    let axes = flags::AXIS_X | flags::AXIS_Y;
    GridParams {
        steps,
        offset,
        clip: clip_end,
        level: grid_level(&steps, dist),
        num_lines: PERSPECTIVE_NUM_LINES,
        grid_flag: flags::SHOW_GRID | flags::PLANE_XY | axes,
        axis_flag: flags::SHOW_AXES | axes,
    }
}

/// Metric steps in a free view at unit scale: 1 m, 10 m, 100 m, ... (`view3d_grid_steps_ex`,
/// whose repeated last step `init_v3d` turns into further tenfold steps).
fn metric_steps() -> [f32; STEPS_LEN] {
    array::from_fn(|level| STEP_GROWTH.powi(level as i32))
}

/// The lowest level whose next step reaches `dist`, plus how far `dist` is toward that step.
fn grid_level(steps: &[f32; STEPS_LEN], dist: f32) -> f32 {
    let last = STEPS_LEN - 1;
    let level = (0..last).find(|&i| steps[i + 1] >= dist).unwrap_or(last);
    let curr = steps[level];
    let next = if level < last {
        steps[level + 1]
    } else {
        curr * STEP_GROWTH
    };
    level as f32 + safe_divide(dist - curr, next - curr)
}

fn safe_divide(a: f32, b: f32) -> f32 {
    if b == 0.0 { 0.0 } else { a / b }
}

/// Grid colours, display sRGB with straight alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridColors {
    pub grid: [f32; 4],
    pub emphasis: [f32; 4],
    pub axes: [[f32; 4]; 3],
}

impl GridColors {
    /// Derived from the default theme as `overlay::Instance::begin_sync` does.
    pub fn from_theme() -> GridColors {
        let grid = theme::shade(theme::VIEW3D_GRID, 10);
        let background = theme::to_float(theme::VIEW3D_BACKGROUND);
        let rgb_sum = |color: [f32; 4]| color[0] + color[1] + color[2];
        // Emphasise lighter rather than darker when the background is darker than the grid.
        let is_background_darker = rgb_sum(grid) + 0.12 > rgb_sum(background);
        let emphasis_offset = if is_background_darker { 20 } else { -10 };
        let axis_offset = ((theme::VIEW3D_GRID_AXIS_BRIGHTNESS * 2.0 - 1.0) * 255.0) as i32;
        GridColors {
            grid,
            emphasis: theme::shade(theme::VIEW3D_GRID_MAJOR, emphasis_offset),
            axes: theme::AXES
                .map(|axis| theme::blend_shade(theme::VIEW3D_GRID, axis, 0.85, axis_offset)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(color: [f32; 4]) -> [u8; 4] {
        color.map(|channel| (channel * 255.0).round() as u8)
    }

    #[test]
    fn theme_colours_match_blender() {
        let colors = GridColors::from_theme();
        assert_eq!(bytes(colors.grid), [94, 94, 94, 128]);
        assert_eq!(bytes(colors.emphasis), [104, 104, 104, 255]);
        assert_eq!(bytes(colors.axes[0]), [209, 35, 62, 235]);
        assert_eq!(bytes(colors.axes[1]), [110, 179, 0, 235]);
    }

    #[test]
    fn level_interpolates_between_steps() {
        let steps = metric_steps();
        assert_eq!(grid_level(&steps, 1.0), 0.0);
        assert_eq!(grid_level(&steps, 10.0), 1.0);
        assert_eq!(grid_level(&steps, 55.0), 1.5);
    }

    #[test]
    fn looking_straight_down_centres_grid_below_camera() {
        let grid = perspective_grid(Vec3::new(3.0, -2.0, 10.0), Vec3::Z, 1000.0);
        assert_eq!(grid.offset, Vec2::new(3.0, -2.0));
        assert_eq!(grid.level, 1.0);
    }

    #[test]
    fn horizontal_view_stays_finite() {
        let grid = perspective_grid(Vec3::new(0.0, -10.0, 2.0), Vec3::NEG_Y, 1000.0);
        assert!(grid.level.is_finite() && grid.offset.is_finite());
    }
}
