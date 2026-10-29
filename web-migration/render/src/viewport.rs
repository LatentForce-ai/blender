//! A 3D viewport bound to one canvas, drawing a blenkernel scene through WebGPU, or WebGL2
//! where WebGPU is unavailable. JavaScript creates it and drives the camera; the top-level
//! `app` crate passes it depsgraph updates and asks it to draw the evaluated scene.

use std::collections::{HashMap, HashSet};

use blenkernel::{Main, Mesh, MeshId, ObjectId};
use depsgraph::{Depsgraph, Updates};
use glam::{Mat4, Vec2};
use shading::ShadingCache;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::camera::{CLIP_END, OrbitCamera};
use crate::environment::{Environment, parse_radiance_hdr};
use crate::grid_pass::GridPass;
use crate::material::{ShadingType, SolidMaterial, sort_back_to_front};
use crate::mesh_extract::{extract_surface, material_slot_count};
use crate::solid_pass::{DEPTH_FORMAT, MeshBatch, ObjectBinding, SolidPass};
use crate::studio_light::StudioLight;
use crate::theme;

/// Multisampling stands in for Blender's overlay and workbench anti-aliasing passes.
const MSAA_SAMPLES: u32 = 4;

/// One material slot of one object.
type SlotKey = (ObjectId, usize);

/// What to draw this frame: opaque slots in any order, transparent slots far to near.
#[derive(Default)]
struct DrawList {
    opaque: Vec<(MeshId, SlotKey)>,
    transparent: Vec<(MeshId, SlotKey)>,
}

/// Per-size render targets: multisampled colour (resolved into the canvas) and depth.
struct FrameTargets {
    color: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
}

#[wasm_bindgen]
pub struct Viewport {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// sRGB view of the canvas texture, so shaders write scene-linear colour
    /// and the hardware applies the display encoding.
    view_format: wgpu::TextureFormat,
    sample_count: u32,
    backend: wgpu::Backend,
    targets: FrameTargets,
    solid: SolidPass,
    grid: GridPass,
    camera: OrbitCamera,
    shading_type: ShadingType,
    meshes: HashMap<MeshId, MeshBatch>,
    objects: HashMap<SlotKey, ObjectBinding>,
}

#[wasm_bindgen]
impl Viewport {
    /// Binds a viewport to `canvas`, sized to the canvas's current pixel size.
    pub async fn create(canvas: HtmlCanvasElement) -> Result<Viewport, JsError> {
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));

        let instance = wgpu::util::new_instance_with_webgpu_detection(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
        .await;
        let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("viewport"),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await?;

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| JsError::new("canvas surface is not supported by the GPU adapter"))?;
        let view_format = config.format.add_srgb_suffix();
        if view_format != config.format {
            config.view_formats.push(view_format);
        }
        surface.configure(&device, &config);
        let sample_count = supported_sample_count(&adapter, view_format);

        Ok(Viewport {
            targets: create_targets(&device, &config, view_format, sample_count),
            solid: SolidPass::new(&device, &queue, view_format, sample_count, &StudioLight::DEFAULT),
            grid: GridPass::new(&device, view_format, sample_count),
            surface,
            device,
            queue,
            config,
            view_format,
            sample_count,
            backend: adapter.get_info().backend,
            camera: OrbitCamera::default(),
            shading_type: ShadingType::default(),
            meshes: HashMap::new(),
            objects: HashMap::new(),
        })
    }

    /// Graphics API in use: `"WebGPU"` or `"WebGL2"`.
    pub fn backend(&self) -> String {
        match self.backend {
            wgpu::Backend::BrowserWebGpu => "WebGPU",
            wgpu::Backend::Gl => "WebGL2",
            _ => "unknown",
        }
        .to_owned()
    }

    /// Matches the drawing surface to the canvas's new pixel size.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.targets = create_targets(
            &self.device,
            &self.config,
            self.view_format,
            self.sample_count,
        );
    }

    /// Orbits the view by a pointer drag, in CSS pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.camera.orbit(dx, dy);
    }

    /// Zooms by mouse-wheel notches; positive moves away, as in Blender.
    pub fn zoom(&mut self, steps: f32) {
        self.camera.zoom(steps);
    }

    /// Lights Material Preview with a Radiance `.hdr` (equirectangular, Blender's world
    /// orientation) instead of the built-in studio environment.
    pub fn set_environment_hdr(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        let env = parse_radiance_hdr(bytes).map_err(|e| JsError::new(&e))?;
        self.solid.set_environment(&self.device, &self.queue, &env);
        Ok(())
    }

    /// Back to the built-in studio environment.
    pub fn reset_environment(&mut self) {
        self.solid.set_environment(&self.device, &self.queue, &Environment::studio());
    }

    /// `"SOLID"` or `"MATERIAL"` (Material Preview), as Blender's `shading.type`.
    pub fn shading_type(&self) -> String {
        self.shading_type.identifier().to_owned()
    }

    /// Switches between Solid (viewport display values) and Material Preview (node-tree
    /// results). Takes effect on the next draw.
    pub fn set_shading_type(&mut self, identifier: &str) -> Result<(), JsError> {
        self.shading_type = ShadingType::from_identifier(identifier)
            .ok_or_else(|| JsError::new(&format!("unsupported shading type \"{identifier}\"")))?;
        Ok(())
    }
}

impl Viewport {
    /// Drops GPU copies of meshes the depsgraph re-evaluated, so the next `draw` re-extracts
    /// them.
    pub fn apply_updates(&mut self, updates: &Updates) {
        for mesh in &updates.geometry {
            self.meshes.remove(mesh);
        }
    }

    /// Draws the depsgraph's evaluated scene and the floor grid into the canvas. Material
    /// Preview reads materials from `shading`. Skips the frame if the canvas is not ready.
    pub fn draw(
        &mut self,
        main: &Main,
        depsgraph: &Depsgraph,
        shading: &ShadingCache,
    ) -> Result<(), JsError> {
        let view = self.camera.view();
        let draws = self.sync_scene(main, depsgraph, shading, view)?;
        let Some(frame) = self.acquire_frame()? else {
            return Ok(());
        };
        let canvas_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let viewport_size = Vec2::new(self.config.width as f32, self.config.height as f32);
        let projection = self.camera.projection(viewport_size.x / viewport_size.y);
        self.solid.set_view(&self.queue, view, projection, self.shading_type);
        self.grid
            .set_view(&self.queue, view, projection, CLIP_END, viewport_size);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = begin_viewport_pass(&mut encoder, &self.targets, &canvas_view);
            let slot = |(mesh, key): &(MeshId, SlotKey)| (&self.meshes[mesh], key.1, &self.objects[key]);
            self.solid.record_opaque(&mut pass, draws.opaque.iter().map(slot));
            self.grid.record(&mut pass);
            // After the grid, so it shows through transparent surfaces.
            self.solid.record_transparent(&mut pass, draws.transparent.iter().map(slot));
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    /// Uploads missing meshes, and evaluated matrices and materials for every material slot of
    /// every object; returns what to draw, with transparent slots sorted for the camera `view`.
    fn sync_scene(
        &mut self,
        main: &Main,
        depsgraph: &Depsgraph,
        shading: &ShadingCache,
        view: Mat4,
    ) -> Result<DrawList, JsError> {
        let mut opaque = Vec::new();
        // View-space depth of the object's origin, as EEVEE sorts blended objects.
        let mut transparent = Vec::new();
        for &object in depsgraph.objects() {
            let Some(mesh) = main.object(object)?.mesh() else {
                continue;
            };
            if !self.ensure_batch(mesh, main.mesh(mesh)?) {
                continue;
            }
            let model = depsgraph
                .world_matrix(object)
                .ok_or_else(|| JsError::new("depsgraph has no matrix for a scene object"))?;
            let depth = (view * model.w_axis).z;
            for slot in 0..self.meshes[&mesh].slot_count() {
                let material = match (self.shading_type, main.object_material(object, slot)?) {
                    (ShadingType::Solid, Some(material)) => SolidMaterial::from(main.material(material)?),
                    (ShadingType::Solid, None) => SolidMaterial::default(),
                    // Empty slots get shading's default surface, as Blender's EEVEE does.
                    (ShadingType::Material, _) => {
                        let shaded = shading.object_slot(main, object, slot);
                        let mut material = SolidMaterial::from(shaded);
                        if let Some(name) = &shaded.base_color_texture {
                            match main.image_by_name(name) {
                                Some(id) => {
                                    self.solid.ensure_texture(&self.device, &self.queue, id, main.image(id)?);
                                    material.texture = Some(id);
                                }
                                // A node naming a missing image: Blender's magenta.
                                None => material.base_color = [1.0, 0.0, 1.0],
                            }
                        }
                        material
                    }
                };
                let binding = self
                    .objects
                    .entry((object, slot))
                    .or_insert_with(|| self.solid.object_binding(&self.device));
                self.solid
                    .set_object(&self.queue, binding, model, &material);
                if material.is_transparent() {
                    transparent.push((depth, (mesh, (object, slot))));
                } else {
                    opaque.push((mesh, (object, slot)));
                }
            }
        }
        self.solid.retain_textures(|id| main.image(id).is_ok());
        sort_back_to_front(&mut transparent);
        let transparent: Vec<_> = transparent.into_iter().map(|(_, draw)| draw).collect();
        let drawn: HashSet<SlotKey> = opaque.iter().chain(&transparent).map(|&(_, key)| key).collect();
        self.objects.retain(|key, _| drawn.contains(key));
        Ok(DrawList { opaque, transparent })
    }

    /// Uploads `mesh` unless its batch is current; false for meshes without triangles. A batch
    /// with a different material count is stale, as in Blender's mesh batch cache.
    fn ensure_batch(&mut self, mesh: MeshId, mesh_data: &Mesh) -> bool {
        let slot_count = material_slot_count(mesh_data);
        if self
            .meshes
            .get(&mesh)
            .is_some_and(|batch| batch.slot_count() == slot_count)
        {
            return true;
        }
        let geometry = extract_surface(mesh_data);
        if geometry.indices.is_empty() {
            self.meshes.remove(&mesh);
            return false;
        }
        self.meshes
            .insert(mesh, MeshBatch::upload(&self.device, &geometry));
        true
    }

    fn acquire_frame(&mut self) -> Result<Option<wgpu::SurfaceTexture>, JsError> {
        use wgpu::CurrentSurfaceTexture as Current;
        match self.surface.get_current_texture() {
            Current::Success(frame) | Current::Suboptimal(frame) => Ok(Some(frame)),
            Current::Timeout | Current::Occluded => Ok(None),
            Current::Outdated => {
                self.surface.configure(&self.device, &self.config);
                Ok(None)
            }
            Current::Lost => Err(JsError::new("canvas surface was lost")),
            Current::Validation => Err(JsError::new("canvas surface texture failed validation")),
        }
    }
}

/// `MSAA_SAMPLES` when both render-target formats support it, otherwise no multisampling.
fn supported_sample_count(adapter: &wgpu::Adapter, color_format: wgpu::TextureFormat) -> u32 {
    let supports = |format| {
        adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(MSAA_SAMPLES)
    };
    if supports(color_format) && supports(DEPTH_FORMAT) {
        MSAA_SAMPLES
    } else {
        1
    }
}

/// Starts the frame's pass: clears colour to the theme background and depth to far, and
/// resolves multisampled colour into the canvas.
fn begin_viewport_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    targets: &FrameTargets,
    canvas_view: &wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
    let [r, g, b, a] = theme::to_linear(theme::to_float(theme::VIEW3D_BACKGROUND)).map(f64::from);
    let (view, resolve_target, store) = match &targets.color {
        Some(multisampled) => (multisampled, Some(canvas_view), wgpu::StoreOp::Discard),
        None => (canvas_view, None, wgpu::StoreOp::Store),
    };
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("viewport"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &targets.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

fn create_targets(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    color_format: wgpu::TextureFormat,
    sample_count: u32,
) -> FrameTargets {
    let target = |label: &str, format: wgpu::TextureFormat| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    FrameTargets {
        color: (sample_count > 1).then(|| target("viewport colour", color_format)),
        depth: target("viewport depth", DEPTH_FORMAT),
    }
}
