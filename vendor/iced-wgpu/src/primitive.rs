//! Draw custom primitives.
use crate::core::{self, Rectangle, Size};
use crate::graphics::Viewport;

use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::fmt::Debug;

/// A batch of primitives.
pub type Batch = Vec<Instance>;

/// A set of methods which allows a [`Primitive`] to be rendered.
pub trait Primitive: Debug + Send + Sync + 'static {
    /// Returns whether this primitive samples the framebuffer behind it.
    fn needs_backdrop(&self) -> bool {
        false
    }

    /// Processes the [`Primitive`], allowing for GPU buffer allocation.
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        storage: &mut Storage,
        bounds: &Rectangle,
        viewport: &Viewport,
    );

    /// Renders the [`Primitive`].
    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        storage: &Storage,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    );
}

/// A sampleable copy of the surface directly below a backdrop primitive.
#[derive(Debug)]
pub struct BackdropTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub size: Size<u32>,
}

impl BackdropTexture {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: Size<u32>,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iced_wgpu.backdrop.snapshot"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self { texture, view, size }
    }
}

#[derive(Debug)]
/// An instance of a specific [`Primitive`].
pub struct Instance {
    /// The bounds of the [`Instance`].
    pub bounds: Rectangle,

    /// The [`Primitive`] to render.
    pub primitive: Box<dyn Primitive>,
}

impl Instance {
    /// Creates a new [`Instance`] with the given [`Primitive`].
    pub fn new(bounds: Rectangle, primitive: impl Primitive) -> Self {
        Instance {
            bounds,
            primitive: Box::new(primitive),
        }
    }
}

/// A renderer than can draw custom primitives.
pub trait Renderer: core::Renderer {
    /// Draws a custom primitive.
    fn draw_primitive(&mut self, bounds: Rectangle, primitive: impl Primitive);
}

/// Stores custom, user-provided types.
#[derive(Default, Debug)]
pub struct Storage {
    pipelines: FxHashMap<TypeId, Box<dyn Any + Send>>,
}

impl Storage {
    /// Returns `true` if `Storage` contains a type `T`.
    pub fn has<T: 'static>(&self) -> bool {
        self.pipelines.contains_key(&TypeId::of::<T>())
    }

    /// Inserts the data `T` in to [`Storage`].
    pub fn store<T: 'static + Send>(&mut self, data: T) {
        let _ = self.pipelines.insert(TypeId::of::<T>(), Box::new(data));
    }

    /// Returns a reference to the data with type `T` if it exists in [`Storage`].
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.pipelines.get(&TypeId::of::<T>()).map(|pipeline| {
            pipeline
                .downcast_ref::<T>()
                .expect("Value with this type does not exist in Storage.")
        })
    }

    /// Returns a mutable reference to the data with type `T` if it exists in [`Storage`].
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.pipelines.get_mut(&TypeId::of::<T>()).map(|pipeline| {
            pipeline
                .downcast_mut::<T>()
                .expect("Value with this type does not exist in Storage.")
        })
    }
}
