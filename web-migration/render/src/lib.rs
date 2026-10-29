//! Browser renderer for the Blender web migration (HACK-3): draws a blenkernel scene into
//! an HTML canvas through WebGPU, falling back to WebGL2 where WebGPU is unavailable.
//!
//! `camera`, `material`, `mesh_extract`, `overlay_grid`, `studio_light` and `theme` are plain
//! Rust and tested natively; the GPU and JavaScript layers only build for `wasm32`.

pub mod camera;
#[cfg(target_arch = "wasm32")]
mod grid_pass;
pub mod material;
pub mod mesh_extract;
pub mod overlay_grid;
#[cfg(target_arch = "wasm32")]
mod solid_pass;
pub mod studio_light;
pub mod theme;
#[cfg(target_arch = "wasm32")]
mod uniforms;
#[cfg(target_arch = "wasm32")]
mod viewport;

#[cfg(target_arch = "wasm32")]
pub use viewport::Viewport;
