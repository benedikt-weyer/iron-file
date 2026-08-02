use bytemuck::{Pod, Zeroable};
use iced::{Rectangle, mouse, widget::shader};
use iced_wgpu::primitive::BackdropTexture;

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

struct Pipeline {
    pipeline: shader::wgpu::RenderPipeline,
    bind_group_layout: shader::wgpu::BindGroupLayout,
    uniform_buffer: shader::wgpu::Buffer,
    bind_group: Option<shader::wgpu::BindGroup>,
}

impl Pipeline {
    fn new(device: &shader::wgpu::Device, format: shader::wgpu::TextureFormat) -> Self {
        let bind_group_layout =
            device.create_bind_group_layout(&shader::wgpu::BindGroupLayoutDescriptor {
                label: Some("iron_file.backdrop_blur.layout"),
                entries: &[
                    shader::wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: shader::wgpu::ShaderStages::FRAGMENT,
                        ty: shader::wgpu::BindingType::Sampler(
                            shader::wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                    shader::wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: shader::wgpu::ShaderStages::FRAGMENT,
                        ty: shader::wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: shader::wgpu::TextureViewDimension::D2,
                            sample_type: shader::wgpu::TextureSampleType::Float {
                                filterable: true,
                            },
                        },
                        count: None,
                    },
                    shader::wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: shader::wgpu::ShaderStages::FRAGMENT,
                        ty: shader::wgpu::BindingType::Buffer {
                            ty: shader::wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout =
            device.create_pipeline_layout(&shader::wgpu::PipelineLayoutDescriptor {
                label: Some("iron_file.backdrop_blur.pipeline_layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let module = device.create_shader_module(shader::wgpu::ShaderModuleDescriptor {
            label: Some("iron_file.backdrop_blur.shader"),
            source: shader::wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&shader::wgpu::RenderPipelineDescriptor {
            label: Some("iron_file.backdrop_blur.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: shader::wgpu::VertexState {
                module: &module,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(shader::wgpu::FragmentState {
                module: &module,
                entry_point: "fs_main",
                targets: &[Some(shader::wgpu::ColorTargetState {
                    format,
                    blend: Some(shader::wgpu::BlendState::REPLACE),
                    write_mask: shader::wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: shader::wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: shader::wgpu::MultisampleState::default(),
            multiview: None,
        });
        let uniform_buffer = device.create_buffer(&shader::wgpu::BufferDescriptor {
            label: Some("iron_file.backdrop_blur.uniform"),
            size: size_of::<BlurUniform>() as u64,
            usage: shader::wgpu::BufferUsages::UNIFORM | shader::wgpu::BufferUsages::COPY_DST,
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
    fn needs_backdrop(&self) -> bool {
        true
    }

    fn prepare(
        &self,
        device: &shader::wgpu::Device,
        queue: &shader::wgpu::Queue,
        format: shader::wgpu::TextureFormat,
        storage: &mut shader::Storage,
        _bounds: &Rectangle,
        viewport: &shader::Viewport,
    ) {
        if !storage.has::<Pipeline>() {
            storage.store(Pipeline::new(device, format));
        }

        let sampler = device.create_sampler(&shader::wgpu::SamplerDescriptor {
            label: Some("iron_file.backdrop_blur.sampler"),
            mag_filter: shader::wgpu::FilterMode::Linear,
            min_filter: shader::wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = {
            let pipeline = storage.get::<Pipeline>().expect("backdrop pipeline");
            let backdrop = storage
                .get::<BackdropTexture>()
                .expect("backdrop texture is installed by the renderer patch");
            device.create_bind_group(&shader::wgpu::BindGroupDescriptor {
                label: Some("iron_file.backdrop_blur.bind_group"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    shader::wgpu::BindGroupEntry {
                        binding: 0,
                        resource: shader::wgpu::BindingResource::Sampler(&sampler),
                    },
                    shader::wgpu::BindGroupEntry {
                        binding: 1,
                        resource: shader::wgpu::BindingResource::TextureView(&backdrop.view),
                    },
                    shader::wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pipeline.uniform_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let pipeline = storage.get_mut::<Pipeline>().expect("backdrop pipeline");
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
        encoder: &mut shader::wgpu::CommandEncoder,
        storage: &shader::Storage,
        target: &shader::wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let pipeline = storage.get::<Pipeline>().expect("backdrop pipeline");
        let bind_group = pipeline.bind_group.as_ref().expect("backdrop bind group");
        let mut pass = encoder.begin_render_pass(&shader::wgpu::RenderPassDescriptor {
            label: Some("iron_file.backdrop_blur.render_pass"),
            color_attachments: &[Some(shader::wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: shader::wgpu::Operations {
                    load: shader::wgpu::LoadOp::Load,
                    store: shader::wgpu::StoreOp::Store,
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
