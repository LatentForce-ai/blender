//! Solid-shading pass with studio lighting: the forward equivalent of workbench's default
//! opaque pipeline (`draw/engines/workbench/workbench_mesh_passes.cc`), with depth test
//! LESS_EQUAL and no back-face culling (`V3D_SHADING_BACKFACE_CULLING` is off by default).
//!
//! Materials with alpha below 1 go through a blended pipeline instead: drawn after the opaque
//! surfaces, far to near, without writing depth, each mesh's back faces before its front faces
//! so a closed mesh doesn't hide its own near side. Workbench uses weighted blended OIT and
//! EEVEE sorts per object; sorting per object matches EEVEE's "Blended" render method.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::material::SolidMaterial;
use crate::mesh_extract::{CornerVertex, SurfaceGeometry};
use crate::studio_light::StudioLight;
use crate::uniforms::{bind_uniforms, uniform_buffer, uniform_layout};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

/// Straight-alpha "over" for colour; alpha accumulates with "over" too, so the canvas stays
/// opaque wherever the opaque pass or the background already made it opaque.
const BLENDED: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent::OVER,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewData {
    view_projection: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
}

/// GPU layout of workbench's `LightData`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightData {
    direction: [f32; 4],
    specular_color: [f32; 4],
    diffuse_color_wrap: [f32; 4],
}

/// GPU layout of the lighting part of workbench's `WorldData`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WorldData {
    lights: [LightData; 4],
    ambient_color: [f32; 4],
    use_specular: u32,
    _padding: [u32; 3],
}

impl WorldData {
    fn new(studio: &StudioLight) -> WorldData {
        WorldData {
            lights: studio.lights.map(|light| LightData {
                direction: light.direction.extend(0.0).to_array(),
                specular_color: light.specular.extend(0.0).to_array(),
                diffuse_color_wrap: light.diffuse.extend(light.smooth).to_array(),
            }),
            ambient_color: studio.ambient.extend(0.0).to_array(),
            use_specular: studio.specular_highlights.into(),
            _padding: [0; 3],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ObjectData {
    model: [[f32; 4]; 4],
    normal_matrix: [[f32; 4]; 4],
    base_color: [f32; 3],
    alpha: f32,
    roughness: f32,
    metallic: f32,
    mirrored: u32,
    _padding: u32,
    /// Material Preview only; WGSL aligns the `vec3` to 16 bytes, which is offset 160 here.
    emission: [f32; 3],
    _padding_emission: f32,
}

/// One mesh's surface batch on the GPU.
pub struct MeshBatch {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    slot_ranges: Vec<Range<u32>>,
}

impl MeshBatch {
    pub fn upload(device: &wgpu::Device, geometry: &SurfaceGeometry) -> MeshBatch {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&geometry.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh triangles"),
            contents: bytemuck::cast_slice(&geometry.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        MeshBatch {
            vertices,
            indices,
            slot_ranges: geometry.slot_ranges.clone(),
        }
    }

    pub fn slot_count(&self) -> usize {
        self.slot_ranges.len()
    }
}

/// Uniforms for one material slot of one object (matrices and material) and their bind group.
pub struct ObjectBinding {
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Negative scale flips the winding, which swaps which faces culling treats as back.
    mirrored: bool,
}

/// Which faces a pipeline draws and how it writes colour and depth.
#[derive(Clone, Copy)]
enum PipelineKind {
    Opaque,
    /// Blended, culling front-facing triangles (draws the far side of a mesh).
    BlendedBackFaces,
    /// Blended, culling back-facing triangles (draws the near side).
    BlendedFrontFaces,
}

pub struct SolidPass {
    opaque: wgpu::RenderPipeline,
    blended_back_faces: wgpu::RenderPipeline,
    blended_front_faces: wgpu::RenderPipeline,
    view_uniform: wgpu::Buffer,
    /// Kept alive for `frame_bind_group`; written once.
    _world_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
}

impl SolidPass {
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        sample_count: u32,
        studio: &StudioLight,
    ) -> SolidPass {
        let frame_layout = uniform_layout(device, "frame", 2);
        let object_layout = uniform_layout(device, "object", 1);
        let layouts = [&frame_layout, &object_layout];
        let pipeline =
            |kind| create_pipeline(device, &layouts, color_format, sample_count, kind);

        let view_uniform = uniform_buffer::<ViewData>(device, "view");
        let world_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world"),
            contents: bytemuck::bytes_of(&WorldData::new(studio)),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let frame_bind_group =
            bind_uniforms(device, &frame_layout, &[&view_uniform, &world_uniform]);
        SolidPass {
            opaque: pipeline(PipelineKind::Opaque),
            blended_back_faces: pipeline(PipelineKind::BlendedBackFaces),
            blended_front_faces: pipeline(PipelineKind::BlendedFrontFaces),
            view_uniform,
            _world_uniform: world_uniform,
            frame_bind_group,
            object_layout,
        }
    }

    pub fn object_binding(&self, device: &wgpu::Device) -> ObjectBinding {
        let uniform = uniform_buffer::<ObjectData>(device, "object");
        let bind_group = bind_uniforms(device, &self.object_layout, &[&uniform]);
        ObjectBinding {
            uniform,
            bind_group,
            mirrored: false,
        }
    }

    pub fn set_view(&self, queue: &wgpu::Queue, view: Mat4, projection: Mat4) {
        let view_data = ViewData {
            view_projection: (projection * view).to_cols_array_2d(),
            view: view.to_cols_array_2d(),
        };
        queue.write_buffer(&self.view_uniform, 0, bytemuck::bytes_of(&view_data));
    }

    pub fn set_object(
        &self,
        queue: &wgpu::Queue,
        binding: &mut ObjectBinding,
        model: Mat4,
        material: &SolidMaterial,
    ) {
        binding.mirrored = model.determinant() < 0.0;
        let object_data = ObjectData {
            model: model.to_cols_array_2d(),
            normal_matrix: model.inverse().transpose().to_cols_array_2d(),
            base_color: material.base_color,
            alpha: material.alpha,
            roughness: material.disney_roughness(),
            metallic: material.metallic,
            mirrored: binding.mirrored.into(),
            _padding: 0,
            emission: material.emission,
            _padding_emission: 0.0,
        };
        queue.write_buffer(&binding.uniform, 0, bytemuck::bytes_of(&object_data));
    }

    /// Draws opaque material slots.
    pub fn record_opaque<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: impl IntoIterator<Item = (&'a MeshBatch, usize, &'a ObjectBinding)>,
    ) {
        pass.set_pipeline(&self.opaque);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        for (batch, slot, binding) in draws {
            Self::draw_slot(pass, batch, slot, binding);
        }
    }

    /// Draws transparent material slots, which must already be sorted far to near.
    pub fn record_transparent<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: impl IntoIterator<Item = (&'a MeshBatch, usize, &'a ObjectBinding)>,
    ) {
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        for (batch, slot, binding) in draws {
            let (far_side, near_side) = if binding.mirrored {
                (&self.blended_front_faces, &self.blended_back_faces)
            } else {
                (&self.blended_back_faces, &self.blended_front_faces)
            };
            for pipeline in [far_side, near_side] {
                pass.set_pipeline(pipeline);
                Self::draw_slot(pass, batch, slot, binding);
            }
        }
    }

    fn draw_slot(pass: &mut wgpu::RenderPass<'_>, batch: &MeshBatch, slot: usize, binding: &ObjectBinding) {
        let range = batch.slot_ranges[slot].clone();
        if range.is_empty() {
            return;
        }
        pass.set_bind_group(1, &binding.bind_group, &[]);
        pass.set_vertex_buffer(0, batch.vertices.slice(..));
        pass.set_index_buffer(batch.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(range, 0, 0..1);
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    bind_group_layouts: &[&wgpu::BindGroupLayout],
    color_format: wgpu::TextureFormat,
    sample_count: u32,
    kind: PipelineKind,
) -> wgpu::RenderPipeline {
    let (blend, cull_mode, depth_write) = match kind {
        PipelineKind::Opaque => (None, None, true),
        PipelineKind::BlendedBackFaces => (Some(BLENDED), Some(wgpu::Face::Front), false),
        PipelineKind::BlendedFrontFaces => (Some(BLENDED), Some(wgpu::Face::Back), false),
    };
    let shader = device.create_shader_module(wgpu::include_wgsl!("solid.wgsl"));
    let bind_group_layouts: Vec<_> = bind_group_layouts.iter().copied().map(Some).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("solid"),
        bind_group_layouts: &bind_group_layouts,
        ..Default::default()
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("solid"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<CornerVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}
