//! Draw custom primitives.
use crate::core::{self, Rectangle, Size};
use crate::graphics::Viewport;
use crate::graphics::futures::{MaybeSend, MaybeSync};

use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::fmt::Debug;

/// A batch of primitives.
pub type Batch = Vec<Instance>;

/// A set of methods which allows a [`Primitive`] to be rendered.
pub trait Primitive: Debug + MaybeSend + MaybeSync + 'static {
    /// Returns whether this primitive samples the framebuffer behind it.
    fn needs_backdrop(&self) -> bool {
        false
    }

    /// The shared renderer of this [`Primitive`].
    ///
    /// Normally, this will contain a bunch of [`wgpu`] state; like
    /// a rendering pipeline, buffers, and textures.
    ///
    /// All instances of this [`Primitive`] type will share the same
    /// [`Renderer`].
    type Pipeline: Pipeline + MaybeSend + MaybeSync;

    /// Processes the [`Primitive`], allowing for GPU buffer allocation.
    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
        backdrop: Option<&BackdropTexture>,
    );

    /// Draws the [`Primitive`] in the given [`wgpu::RenderPass`].
    ///
    /// When possible, this should be implemented over [`render`](Self::render)
    /// since reusing the existing render pass should be considerably more
    /// efficient than issuing a new one.
    ///
    /// The viewport and scissor rect of the render pass provided is set
    /// to the bounds and clip bounds of the [`Primitive`], respectively.
    ///
    /// If you have complex composition needs, then you can leverage
    /// [`render`](Self::render) by returning `false` here.
    ///
    /// By default, it does nothing and returns `false`.
    fn draw(
        &self,
        _pipeline: &Self::Pipeline,
        _render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        false
    }

    /// Renders the [`Primitive`], using the given [`wgpu::CommandEncoder`].
    ///
    /// This will only be called if [`draw`](Self::draw) returns `false`.
    ///
    /// By default, it does nothing.
    fn render(
        &self,
        _pipeline: &Self::Pipeline,
        _encoder: &mut wgpu::CommandEncoder,
        _target: &wgpu::TextureView,
        _clip_bounds: &Rectangle<u32>,
    ) {
    }
}

/// The pipeline of a graphics [`Primitive`].
pub trait Pipeline: Any + MaybeSend + MaybeSync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Creates the [`Pipeline`] of a [`Primitive`].
    ///
    /// This will only be called once, when the first [`Primitive`] with this kind
    /// of [`Pipeline`] is encountered.
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self
    where
        Self: Sized;

    /// Trims any cached data in the [`Pipeline`].
    ///
    /// This will normally be called at the end of a frame.
    fn trim(&mut self) {}
}

impl dyn Pipeline {
    fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut()
    }
}

pub(crate) trait Stored:
    Debug + MaybeSend + MaybeSync + 'static
{
    fn needs_backdrop(&self) -> bool;

    fn prepare(
        &self,
        storage: &mut Storage,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        bounds: &Rectangle,
        viewport: &Viewport,
    );

    fn draw(
        &self,
        storage: &Storage,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool;

    fn render(
        &self,
        storage: &Storage,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    );
}

#[derive(Debug)]
struct BlackBox<P: Primitive> {
    primitive: P,
}

impl<P: Primitive> Stored for BlackBox<P> {
    fn needs_backdrop(&self) -> bool {
        self.primitive.needs_backdrop()
    }

    fn prepare(
        &self,
        storage: &mut Storage,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        if self.primitive.needs_backdrop() {
            let size = viewport.physical_size();
            let replace = storage
                .backdrop
                .as_ref()
                .is_none_or(|backdrop| backdrop.size != size);

            if replace {
                storage.backdrop = Some(BackdropTexture::new(device, format, size));
            }
        }

        if !storage.has::<P>() {
            storage.store::<P, _>(P::Pipeline::new(device, queue, format));
        }

        let backdrop = storage.backdrop.as_ref();
        let renderer = storage
            .pipelines
            .get_mut(&TypeId::of::<P>())
            .expect("renderer should be initialized")
            .downcast_mut::<P::Pipeline>()
            .expect("renderer should have the proper type");

        self.primitive
            .prepare(renderer, device, queue, bounds, viewport, backdrop);
    }

    fn draw(
        &self,
        storage: &Storage,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let renderer = storage
            .get::<P>()
            .expect("renderer should be initialized")
            .downcast_ref::<P::Pipeline>()
            .expect("renderer should have the proper type");

        self.primitive.draw(renderer, render_pass)
    }

    fn render(
        &self,
        storage: &Storage,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let renderer = storage
            .get::<P>()
            .expect("renderer should be initialized")
            .downcast_ref::<P::Pipeline>()
            .expect("renderer should have the proper type");

        self.primitive
            .render(renderer, encoder, target, clip_bounds);
    }
}

#[derive(Debug)]
/// An instance of a specific [`Primitive`].
pub struct Instance {
    /// The bounds of the [`Instance`].
    pub(crate) bounds: Rectangle,

    /// The [`Primitive`] to render.
    pub(crate) primitive: Box<dyn Stored>,
}

impl Instance {
    /// Creates a new [`Instance`] with the given [`Primitive`].
    pub fn new(bounds: Rectangle, primitive: impl Primitive) -> Self {
        Instance {
            bounds,
            primitive: Box::new(BlackBox { primitive }),
        }
    }
}

/// A renderer than can draw custom primitives.
pub trait Renderer: core::Renderer {
    /// Draws a custom primitive.
    fn draw_primitive(&mut self, bounds: Rectangle, primitive: impl Primitive);
}

/// Stores custom, user-provided types.
#[derive(Default)]
pub struct Storage {
    pipelines: FxHashMap<TypeId, Box<dyn Pipeline>>,
    backdrop: Option<BackdropTexture>,
}

impl Storage {
    /// Returns `true` if `Storage` contains a type `T`.
    pub fn has<T: 'static>(&self) -> bool {
        self.pipelines.contains_key(&TypeId::of::<T>())
    }

    /// Inserts the data `T` in to [`Storage`].
    pub fn store<T: 'static, P: Pipeline>(&mut self, pipeline: P) {
        let _ = self.pipelines.insert(TypeId::of::<T>(), Box::new(pipeline));
    }

    /// Returns the current backdrop snapshot, if one was requested.
    pub fn backdrop(&self) -> Option<&BackdropTexture> {
        self.backdrop.as_ref()
    }

    /// Replaces the current backdrop snapshot.
    pub fn set_backdrop(&mut self, backdrop: BackdropTexture) {
        self.backdrop = Some(backdrop);
    }

    /// Returns a reference to the data with type `T` if it exists in [`Storage`].
    pub fn get<T: 'static>(&self) -> Option<&dyn Any> {
        self.pipelines
            .get(&TypeId::of::<T>())
            .map(|pipeline| pipeline.as_ref() as &dyn Any)
    }

    /// Returns a mutable reference to the data with type `T` if it exists in [`Storage`].
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut dyn Any> {
        self.pipelines
            .get_mut(&TypeId::of::<T>())
            .map(|pipeline| pipeline.as_mut() as &mut dyn Any)
    }

    /// Trims the cache of all the pipelines in the [`Storage`].
    pub fn trim(&mut self) {
        for pipeline in self.pipelines.values_mut() {
            pipeline.trim();
        }
    }
}

/// A sampleable copy of the surface behind a backdrop primitive.
#[derive(Debug)]
pub struct BackdropTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub size: Size<u32>,
}

impl BackdropTexture {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: Size<u32>) -> Self {
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
