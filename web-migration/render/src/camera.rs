//! Orbit camera for the free 3D viewport, using Blender's `View3D` defaults
//! (`makesdna/DNA_view3d_types.h`) and its perspective viewplane
//! (`BKE_camera_params_from_view3d` in `blenkernel/intern/camera.cc`).

use std::f32::consts::FRAC_PI_2;

use glam::{Mat4, Vec3};

/// `View3D::lens`, in millimetres.
const LENS_MM: f32 = 50.0;
/// `DEFAULT_SENSOR_WIDTH` (36 mm) times `CAMERA_PARAM_ZOOM_INIT_PERSP` (2): the free
/// viewport's larger side spans this much sensor.
const SENSOR_SPAN_MM: f32 = 72.0;
const CLIP_START: f32 = 0.01;
pub const CLIP_END: f32 = 1000.0;

const ORBIT_RADIANS_PER_PIXEL: f32 = 0.008;
/// Distance multiplier per zoom step (one mouse-wheel notch).
const ZOOM_STEP: f32 = 1.2;
const MIN_DISTANCE: f32 = 0.1;
const MAX_DISTANCE: f32 = 500.0;
/// Stops just short of straight up/down so the Z-up basis stays defined.
const PITCH_LIMIT: f32 = FRAC_PI_2 - 1e-3;

/// Turntable camera orbiting `target` around world Z, Blender's default orbit style.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    target: Vec3,
    /// Rotation around world Z, in radians.
    yaw: f32,
    /// Elevation above the XY plane, in radians.
    pitch: f32,
    distance: f32,
}

impl Default for OrbitCamera {
    /// Looks at the origin from front-right and above, as Blender's startup file does.
    fn default() -> Self {
        OrbitCamera {
            target: Vec3::ZERO,
            yaw: (-45f32).to_radians(),
            pitch: 30f32.to_radians(),
            distance: 10.0,
        }
    }
}

impl OrbitCamera {
    pub fn eye(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        self.target + self.distance * Vec3::new(cos_pitch * cos_yaw, cos_pitch * sin_yaw, sin_pitch)
    }

    /// World-to-view matrix (right-handed, camera looks down -Z).
    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Z)
    }

    /// View-to-clip matrix with WebGPU's [0, 1] depth range.
    pub fn projection(&self, aspect: f32) -> Mat4 {
        let tan_half_major = SENSOR_SPAN_MM / 2.0 / LENS_MM;
        let tan_half_y = if aspect >= 1.0 {
            tan_half_major / aspect
        } else {
            tan_half_major
        };
        Mat4::perspective_rh(2.0 * tan_half_y.atan(), aspect, CLIP_START, CLIP_END)
    }

    /// Orbits by a mouse drag in pixels; the scene follows the pointer.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * ORBIT_RADIANS_PER_PIXEL;
        self.pitch = (self.pitch + dy * ORBIT_RADIANS_PER_PIXEL).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Moves toward (negative `steps`) or away from (positive) the target.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * ZOOM_STEP.powf(steps)).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec4;

    use super::*;

    fn to_ndc(clip: Vec4) -> Vec3 {
        clip.truncate() / clip.w
    }

    #[test]
    fn larger_side_spans_blender_viewplane() {
        let camera = OrbitCamera::default();
        let depth = 5.0;
        let edge = SENSOR_SPAN_MM / 2.0 / LENS_MM * depth;
        let wide = camera.projection(16.0 / 9.0) * Vec4::new(edge, 0.0, -depth, 1.0);
        let tall = camera.projection(9.0 / 16.0) * Vec4::new(0.0, edge, -depth, 1.0);
        assert!((to_ndc(wide).x - 1.0).abs() < 1e-5);
        assert!((to_ndc(tall).y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn depth_range_is_zero_to_one() {
        let projection = OrbitCamera::default().projection(1.0);
        let near = to_ndc(projection * Vec4::new(0.0, 0.0, -CLIP_START, 1.0));
        let far = to_ndc(projection * Vec4::new(0.0, 0.0, -CLIP_END, 1.0));
        assert!(near.z.abs() < 1e-5);
        assert!((far.z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn target_is_in_front_of_camera() {
        let camera = OrbitCamera::default();
        let target_in_view = camera.view().transform_point3(camera.target);
        assert!(target_in_view.x.abs() < 1e-4 && target_in_view.y.abs() < 1e-4);
        assert!((target_in_view.z + camera.distance).abs() < 1e-4);
    }

    #[test]
    fn orbit_clamps_pitch_and_zoom_clamps_distance() {
        let mut camera = OrbitCamera::default();
        camera.orbit(0.0, 1e6);
        camera.zoom(-1e3);
        assert_eq!(camera.pitch, PITCH_LIMIT);
        assert_eq!(camera.distance, MIN_DISTANCE);
    }
}
