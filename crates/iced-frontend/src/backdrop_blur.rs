use bytemuck::{Pod, Zeroable};
use iced::{Rectangle, mouse, widget::shader};
use iced_wgpu::{primitive::BackdropTexture, wgpu};

const SHADER: &str = r#"
struct BlurUniform {
    texture_size: vec2<f32>,
    strength: f32,
    kernel_size: u32,
};

@group(0) @binding(0) var source_sampler: sampler;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> blur: BlurUniform;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / blur.texture_size;
    let texel = 1.0 / blur.texture_size;
    let half_kernel = i32(blur.kernel_size / 2u);
    let sigma = max(blur.strength, 0.5);
    let sample_offset = texel * (3.0 * sigma / f32(half_kernel));
    var color = vec4<f32>(0.0);
    var total_weight = 0.0;
    for (var y = -15; y <= 15; y += 1) {
        for (var x = -15; x <= 15; x += 1) {
            if (abs(x) <= half_kernel && abs(y) <= half_kernel) {
                let distance_squared = f32(x * x + y * y);
                let weight = exp(-distance_squared / (2.0 * sigma * sigma));
                let offset = vec2<f32>(f32(x), f32(y)) * sample_offset;
                color += textureSample(source_texture, source_sampler, uv + offset) * weight;
                total_weight += weight;
            }
        }
    }
    return color / total_weight;
}
"#;

#[derive(Debug, Clone, Copy)]
pub struct BackdropBlur {
    strength: f32,
    kernel_size: u32,
}

impl BackdropBlur {
    pub fn new(strength: u8, kernel_size: u8) -> Self {
        Self {
            strength: f32::from(strength),
            kernel_size: u32::from(kernel_size),
        }
    }
}

impl<Message> shader::Program<Message> for BackdropBlur {
    type State = ();
    type Primitive = BlurPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        BlurPrimitive {
            strength: self.strength,
            kernel_size: self.kernel_size,
        }
    }
}

#[derive(Debug)]
pub struct BlurPrimitive {
    strength: f32,
    kernel_size: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurUniform {
    texture_size: [f32; 2],
    strength: f32,
    kernel_size: u32,
}

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
}

impl shader::Pipeline for Pipeline {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iron_file.backdrop_blur.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iron_file.backdrop_blur.pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iron_file.backdrop_blur.shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iron_file.backdrop_blur.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iron_file.backdrop_blur.uniform"),
            size: size_of::<BlurUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            bind_group: None,
        }
    }
}

impl shader::Primitive for BlurPrimitive {
    type Pipeline = Pipeline;

    fn needs_backdrop(&self) -> bool {
        true
    }

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        viewport: &shader::Viewport,
        backdrop: Option<&BackdropTexture>,
    ) {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iron_file.backdrop_blur.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = {
            let backdrop = backdrop.expect("backdrop texture is installed by the renderer patch");
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iron_file.backdrop_blur.bind_group"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&backdrop.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pipeline.uniform_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        queue.write_buffer(
            &pipeline.uniform_buffer,
            0,
            bytemuck::bytes_of(&BlurUniform {
                texture_size: [
                    viewport.physical_size().width.max(1) as f32,
                    viewport.physical_size().height.max(1) as f32,
                ],
                strength: self.strength,
                kernel_size: self.kernel_size,
            }),
        );
        pipeline.bind_group = Some(bind_group);
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let bind_group = pipeline.bind_group.as_ref().expect("backdrop bind group");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iron_file.backdrop_blur.render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::SHADER;

    #[test]
    fn shader_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("valid blur shader");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("valid blur shader module");
    }
}
