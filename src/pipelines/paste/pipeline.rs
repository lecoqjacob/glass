use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use wgpu::{
    util::DeviceExt, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, ColorTargetState, ColorWrites,
    CommandEncoder, Device, LoadOp, Operations, PushConstantRange, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, SamplerBindingType, ShaderStages, TextureFormat,
    TextureSampleType, TextureViewDimension,
};

use crate::{
    pipelines::{TexturedVertex, QUAD_INDICES, TEXTURED_QUAD_VERTICES},
    texture::Texture,
};

const PASTE_TEXTURE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

pub struct PastePipeline {
    paste_pipeline: RenderPipeline,
    vertices: Buffer,
    indices: Buffer,
}

impl PastePipeline {
    pub fn new(device: &Device) -> PastePipeline {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Paste Vertex Buffer"),
            contents: bytemuck::cast_slice(
                &TEXTURED_QUAD_VERTICES
                    .iter()
                    .map(|v| TexturedVertex {
                        position: [
                            v.position[0] * 2.0,
                            v.position[1] * 2.0,
                            v.position[2],
                            v.position[3],
                        ],
                        ..*v
                    })
                    .collect::<Vec<TexturedVertex>>(),
            ),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Paste Index Buffer"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        // Bind group layout
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("paste_bind_group_layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float {
                            filterable: false,
                        },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    visibility: ShaderStages::FRAGMENT,
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    ty: BindingType::Sampler(SamplerBindingType::NonFiltering),
                    visibility: ShaderStages::FRAGMENT,
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Paste Shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("paste.wgsl"))),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Paste Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[PushConstantRange {
                stages: ShaderStages::VERTEX_FRAGMENT,
                range: 0..std::mem::size_of::<PastePushConstants>() as u32,
            }],
        });
        let paste_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Paste Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[TexturedVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fragment",
                targets: &[Some(ColorTargetState {
                    format: PASTE_TEXTURE_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent::OVER,
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        PastePipeline {
            paste_pipeline,
            vertices,
            indices,
        }
    }

    pub fn paste(
        &self,
        device: &Device,
        encoder: &mut CommandEncoder,
        input: &Texture,
        output: &Texture,
        tint: [f32; 4],
        size: Vec2,
        offset: Vec2,
        flip_x: bool,
        flip_y: bool,
    ) {
        let image_size = Vec2::new(size.x / output.size[0], size.y / output.size[1]);
        let push_constants: PastePushConstants = PastePushConstants {
            tint,
            scale: [
                image_size.x * if flip_x { -1.0 } else { 1.0 },
                image_size.y * if flip_y { -1.0 } else { 1.0 },
            ],
            offset: [
                (2.0 * offset.x - output.size[0]) / output.size[0],
                -(2.0 * offset.y - output.size[1]) / output.size[1],
            ],
        };
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("paste_bind_group"),
            layout: &self.paste_pipeline.get_bind_group_layout(0),
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&input.views[0]),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&input.sampler),
                },
            ],
        });
        {
            let mut r_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("paste_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &output.views[0],
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        ..Default::default()
                    },
                })],
                depth_stencil_attachment: None,
            });
            r_pass.set_pipeline(&self.paste_pipeline);
            r_pass.set_bind_group(0, &bind_group, &[]);
            r_pass.set_vertex_buffer(0, self.vertices.slice(..));
            r_pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
            r_pass.set_push_constants(
                ShaderStages::VERTEX_FRAGMENT,
                0,
                bytemuck::cast_slice(&[push_constants]),
            );
            r_pass.draw_indexed(0..(QUAD_INDICES.len() as u32), 0, 0..1);
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct PastePushConstants {
    tint: [f32; 4],
    scale: [f32; 2],
    offset: [f32; 2],
}
