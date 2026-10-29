//! Floor grid overlay pass, drawn over the solid pass with alpha blending and a depth test
//! against the scene (`overlay::Grid` in `draw/engines/overlay/overlay_grid.hh`). Blender's
//! first grid iteration also writes depth for overlays drawn after it; nothing is, here.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2};

use crate::overlay_grid::{self, GridColors, ITER_LEN, STEPS_LEN};
use crate::solid_pass::DEPTH_FORMAT;
use crate::theme;
use crate::uniforms::{bind_uniforms, uniform_buffer, uniform_layout};

/// GPU layout of `GridData` in `grid.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GridData {
    view_projection: [[f32; 4]; 4],
    view_position: [f32; 4],
    steps: [[f32; 4]; STEPS_LEN],
    color_grid: [f32; 4],
    color_emphasis: [f32; 4],
    color_axes: [[f32; 4]; 3],
    offset: [f32; 2],
    clip_rect: [f32; 2],
    level: f32,
    num_lines: u32,
    grid_flag: u32,
    axis_flag: u32,
    viewport_size: [f32; 2],
    _padding: [f32; 2],
}

pub struct GridPass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Scene-linear, since the canvas view applies the sRGB encoding.
    colors: GridColors,
    vertex_count: u32,
}

impl GridPass {
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> GridPass {
        let layout = uniform_layout(device, "grid", 1);
        let uniform = uniform_buffer::<GridData>(device, "grid");
        let theme_colors = GridColors::from_theme();
        GridPass {
            pipeline: create_pipeline(device, &layout, color_format, sample_count),
            bind_group: bind_uniforms(device, &layout, &[&uniform]),
            uniform,
            colors: GridColors {
                grid: theme::to_linear(theme_colors.grid),
                emphasis: theme::to_linear(theme_colors.emphasis),
                axes: theme_colors.axes.map(theme::to_linear),
            },
            vertex_count: 0,
        }
    }

    /// Lays the grid out for the camera: its level, offset and line count.
    pub fn set_view(
        &mut self,
        queue: &wgpu::Queue,
        view: Mat4,
        projection: Mat4,
        clip_end: f32,
        viewport_size: Vec2,
    ) {
        let view_inverse = view.inverse();
        let view_position = view_inverse.w_axis.truncate();
        let params =
            overlay_grid::perspective_grid(view_position, view_inverse.z_axis.truncate(), clip_end);
        self.vertex_count = params.vertex_count();
        let grid_data = GridData {
            view_projection: (projection * view).to_cols_array_2d(),
            view_position: view_position.extend(clip_end).to_array(),
            steps: params.steps.map(|step| [step; 4]),
            color_grid: self.colors.grid,
            color_emphasis: self.colors.emphasis,
            color_axes: self.colors.axes,
            offset: params.offset.to_array(),
            clip_rect: [params.clip; 2],
            level: params.level,
            num_lines: params.num_lines,
            grid_flag: params.grid_flag,
            axis_flag: params.axis_flag,
            viewport_size: viewport_size.to_array(),
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&grid_data));
    }

    pub fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..self.vertex_count, 0..ITER_LEN);
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    color_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::include_wgsl!("grid.wgsl"));
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("grid"),
        bind_group_layouts: &[Some(layout)],
        ..Default::default()
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("grid"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
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
