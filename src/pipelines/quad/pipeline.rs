use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};
use wgpu::{
    util::DeviceExt, BindGroup, Buffer, Device, PushConstantRange, RenderPass, RenderPipeline,
    Sampler, ShaderStages, TextureView,
};

use crate::pipelines::{vertex::TexturedVertex, QUAD_INDICES, TEXTURED_QUAD_VERTICES};

pub struct QuadPipeline {
    pipeline: RenderPipeline,
    vertices: Buffer,
    indices: Buffer,
}

impl QuadPipeline {
    pub fn new(device: &Device, color_target_state: wgpu::ColorTargetState) -> QuadPipeline {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(TEXTURED_QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        let pipeline = Self::new_render_pipeline(device, color_target_state);
        Self {
            pipeline,
            vertices,
            indices,
        }
    }

    pub fn new_render_pipeline(
        device: &Device,
        color_target_state: wgpu::ColorTargetState,
    ) -> RenderPipeline {
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float {
                                filterable: true,
                            },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("quad.wgsl"))),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Quad Pipeline Layout"),
            bind_group_layouts: &[&texture_bind_group_layout],
            push_constant_ranges: &[PushConstantRange {
                stages: ShaderStages::VERTEX_FRAGMENT,
                range: 0..std::mem::size_of::<QuadPushConstants>() as u32,
            }],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Quad Render Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[TexturedVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(color_target_state)],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // No cull mode to enable flipping of the quad
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });
        pipeline
    }

    pub fn push_constants(
        view_position: [f32; 4],
        view_proj: [[f32; 4]; 4],
        quad_size: [f32; 2],
        aa_strength: f32,
    ) -> QuadPushConstants {
        QuadPushConstants {
            view_position,
            view_proj,
            dims: quad_size,
            aa_strength,
        }
    }

    pub fn create_bind_group(
        &self,
        device: &Device,
        image: &TextureView,
        sampler: &Sampler,
    ) -> BindGroup {
        let bind_group_layout = self.pipeline.get_bind_group_layout(0);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(image),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
            label: Some("bind_group"),
        });
        bind_group
    }

    pub fn draw<'r>(
        &'r self,
        rpass: &mut RenderPass<'r>,
        bind_group: &'r BindGroup,
        view_pos: [f32; 4],
        view_proj: [[f32; 4]; 4],
        quad_size: [f32; 2],
        aa_strength: f32,
    ) {
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertices.slice(..));
        rpass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        rpass.set_push_constants(
            ShaderStages::VERTEX_FRAGMENT,
            0,
            bytemuck::cast_slice(&[QuadPipeline::push_constants(
                view_pos,
                view_proj,
                quad_size,
                aa_strength,
            )]),
        );
        rpass.draw_indexed(0..(QUAD_INDICES.len() as u32), 0, 0..1);
    }
}

/// Quad instance specific values passed to the shader.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct QuadPushConstants {
    pub view_position: [f32; 4],
    pub view_proj: [[f32; 4]; 4],
    pub dims: [f32; 2],
    pub aa_strength: f32,
}
