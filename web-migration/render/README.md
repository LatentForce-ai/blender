# render (Rust, WebGPU)

The 3D viewport of the Blender web migration (HACK-3): draws the depsgraph's evaluated scene
(object list and world matrices) with blenkernel's meshes and materials, the way Blender's default
Solid viewport does, through WebGPU, falling back to WebGL2. It is a library; build and
run it through [`../app`](../app/README.md).

Instead of porting `source/blender/gpu` and `draw` (about 384k lines) wholesale, this ports the
parts the default viewport uses, onto [`wgpu`](https://wgpu.rs).

## Layout

| File | Ported from (`source/blender/…`) |
|---|---|
| `src/mesh_extract.rs` | `draw/intern/draw_cache_impl_mesh.cc` (surface batch), `SortedFaceData` |
| `src/solid_pass.rs`, `src/solid.wgsl` | `draw/engines/workbench/` opaque and transparent pipelines, `get_world_lighting` |
| `src/studio_light.rs` | `blenkernel/intern/studiolight.cc` (`BKE_studiolight_default`) |
| `src/material.rs` | `workbench::Material` (viewport display values); Material Preview values from `../shading` |
| `src/overlay_grid.rs`, `src/grid_pass.rs`, `src/grid.wgsl` | `draw/engines/overlay/overlay_grid.hh`, `overlay_grid_vert/frag.glsl` |
| `src/theme.rs` | `userdef_default_theme.c`, `ui::theme` shade and blend helpers |
| `src/camera.rs` | `View3D` defaults, `BKE_camera_params_from_view3d` |
| `src/viewport.rs`, `src/uniforms.rs` | new: canvas, render targets, JavaScript API |

`cargo test` checks the plain-Rust modules natively: camera maths, mesh extraction, material
grouping, and grid levels and colours against Blender's formulas. The GPU modules only build for
`wasm32`.

## Known differences from Blender

- One forward pass instead of workbench's G-buffer and resolve passes. With default settings
  (no cavity, shadow, outline or X-ray) the image is the same.
- 4× MSAA instead of workbench's TAA and SMAA and the overlay line anti-aliasing pass.
- Solid mode and a simplified Material Preview (`set_shading_type("MATERIAL")`): node-tree results
  from `../shading` (base colour, metallic, roughness, plus emission) lit by the same studio
  light, instead of EEVEE with an HDRI. No image textures yet.
- A perspective free view with Material colour. No looking through the scene camera and no
  orthographic views.
- Alpha below 1 is drawn blended after opaque surfaces and the grid, sorted far to near per
  object (like EEVEE's "Blended" method), each mesh back faces first. Workbench uses weighted
  blended OIT instead, so intersecting transparent objects can sort differently.
- Overlays: the floor grid and X/Y axes only. No selection outlines, wireframes, origins, or
  camera and light icons.
- Overlay colours blend in scene-linear space rather than display space.
- Normals are uploaded as 32-bit floats; Blender packs them into 10 bits.
- The default theme and studio light are built in rather than read from user preferences.
