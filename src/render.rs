//! wgpu renderer. One shared device + pipeline; one lightweight surface per
//! monitor. Everything is drawn as instanced textured quads in premultiplied
//! alpha so the swapchain can be composited transparently onto the desktop.

use crate::creature::animation::Sprites;
use std::sync::Arc;
use winit::window::Window;

pub const MAX_INSTANCES: usize = 64;

/// One quad to draw, in *window-local* pixel coordinates.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub rect: [f32; 4],  // x, y, w, h
    pub uv: [f32; 4],    // u, v, uw, vh
    pub color: [f32; 4], // tint (straight alpha)
    pub misc: [f32; 4],  // x = flip flag, y = rotation (radians)
}

impl Instance {
    pub fn new(rect: [f32; 4], uv: [f32; 4], color: [f32; 4], flip: bool) -> Self {
        Self::angled(rect, uv, color, flip, 0.0)
    }
    pub fn angled(rect: [f32; 4], uv: [f32; 4], color: [f32; 4], flip: bool, angle: f32) -> Self {
        Self { rect, uv, color, misc: [if flip { 1.0 } else { 0.0 }, angle, 0.0, 0.0] }
    }
}

struct Tex {
    bind_group: wgpu::BindGroup,
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    _adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    tex_bgl: wgpu::BindGroupLayout,
    uni_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pub format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    atlas: Tex,
    pet: Option<(Tex, u32, u32)>,
    banner: Option<(Tex, u32, u32)>,
    banner_key: String,
}

pub struct WindowRender {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    uni_buf: wgpu::Buffer,
    uni_bg: wgpu::BindGroup,
    inst_buf: wgpu::Buffer,
}

impl Gpu {
    /// Build the GPU context using `first` to probe surface capabilities, and
    /// return a renderer for that first window too.
    pub fn new(first: Arc<Window>, sprites: &Sprites) -> (Gpu, WindowRender) {
        let mut idesc = wgpu::InstanceDescriptor::new_without_display_handle();
        idesc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(idesc);
        let surface = instance.create_surface(first.clone()).expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no suitable GPU adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("companion-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..Default::default()
        }))
        .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let alpha_mode = pick_alpha(&caps.alpha_modes);
        eprintln!(
            "[companion] gpu: {} | format {:?} | alpha {:?}",
            adapter.get_info().name,
            format,
            alpha_mode
        );
        if alpha_mode == wgpu::CompositeAlphaMode::Opaque {
            eprintln!("[companion] warning: compositor doesn't advertise per-pixel alpha; \
                       the window background may not be transparent on this setup.");
        }

        // --- bind group layouts --------------------------------------------
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tex-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        });
        let uni_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uni-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // --- pipeline -------------------------------------------------------
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pl"),
            bind_group_layouts: &[Some(&tex_bgl), Some(&uni_bgl)],
            immediate_size: 0,
        });
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let atlas = upload_texture(
            &device,
            &queue,
            &tex_bgl,
            &sampler,
            &sprites.pixels,
            sprites.width,
            sprites.height,
        );

        let gpu = Gpu {
            instance,
            _adapter: adapter,
            device,
            queue,
            pipeline,
            tex_bgl,
            uni_bgl,
            sampler,
            format,
            alpha_mode,
            atlas: Tex { bind_group: atlas },
            pet: None,
            banner: None,
            banner_key: String::new(),
        };

        let first_size = first.inner_size();
        let wr = gpu.make_window_render(first, surface, first_size.width.max(1), first_size.height.max(1));
        (gpu, wr)
    }

    /// Add another monitor's window to the same device.
    pub fn add_window(&mut self, window: Arc<Window>) -> WindowRender {
        let surface = self.instance.create_surface(window.clone()).expect("surface");
        let size = window.inner_size();
        self.make_window_render(window, surface, size.width.max(1), size.height.max(1))
    }

    fn make_window_render(
        &self,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> WindowRender {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: self.alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&self.device, &config);

        let uni_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uni_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uni-bg"),
            layout: &self.uni_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uni_buf.as_entire_binding() }],
        });
        let inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (std::mem::size_of::<Instance>() * MAX_INSTANCES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        WindowRender { window, surface, config, uni_buf, uni_bg, inst_buf }
    }

    /// Upload a custom pet texture (straight-alpha RGBA). Replaces the built-in
    /// procedural creature as the body sprite.
    pub fn set_pet(&mut self, pixels: &[u8], width: u32, height: u32) {
        let tex = upload_texture(&self.device, &self.queue, &self.tex_bgl, &self.sampler, pixels, width, height);
        self.pet = Some((Tex { bind_group: tex }, width, height));
    }

    /// Re-rasterise the banner texture only when the text actually changes.
    pub fn set_banner(&mut self, text: Option<&str>) {
        match text {
            None => {
                self.banner = None;
                self.banner_key.clear();
            }
            Some(t) if t != self.banner_key => {
                let b = crate::banner::Banner::render(t);
                let tex = upload_texture(
                    &self.device,
                    &self.queue,
                    &self.tex_bgl,
                    &self.sampler,
                    &b.pixels,
                    b.width,
                    b.height,
                );
                self.banner = Some((Tex { bind_group: tex }, b.width, b.height));
                self.banner_key = t.to_string();
            }
            _ => {}
        }
    }

    pub fn banner_size(&self) -> Option<(u32, u32)> {
        self.banner.as_ref().map(|(_, w, h)| (*w, *h))
    }
}

impl WindowRender {
    pub fn resize(&mut self, gpu: &Gpu, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Draw, in order: `atlas` quads (shadow/built-in creature/emotes), an
    /// optional `pet` body quad (from the custom pet texture), and an optional
    /// `banner` quad — each from its own texture/bind-group.
    pub fn render(&self, gpu: &Gpu, atlas: &[Instance], pet: Option<Instance>, banner: Option<Instance>) {
        let mut all: Vec<Instance> = Vec::with_capacity(atlas.len() + 2);
        all.extend_from_slice(&atlas[..atlas.len().min(MAX_INSTANCES - 2)]);
        let atlas_count = all.len() as u32;
        let pet_has = pet.is_some() && gpu.pet.is_some();
        if let Some(p) = pet {
            all.push(p);
        }
        let banner_has = banner.is_some() && gpu.banner.is_some();
        if let Some(b) = banner {
            all.push(b);
        }
        gpu.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&all));

        let vp = [self.config.width as f32, self.config.height as f32, 0.0, 0.0];
        gpu.queue.write_buffer(&self.uni_buf, 0, bytemuck::cast_slice(&vp));

        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(t) | Cst::Suboptimal(t) => t,
            _ => {
                self.surface.configure(&gpu.device, &self.config);
                match self.surface.get_current_texture() {
                    Cst::Success(t) | Cst::Suboptimal(t) => t,
                    _ => return,
                }
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(1, &self.uni_bg, &[]);
            pass.set_vertex_buffer(0, self.inst_buf.slice(..));

            if atlas_count > 0 {
                pass.set_bind_group(0, &gpu.atlas.bind_group, &[]);
                pass.draw(0..6, 0..atlas_count);
            }
            let mut next = atlas_count;
            if pet_has {
                if let Some((tex, _, _)) = gpu.pet.as_ref() {
                    pass.set_bind_group(0, &tex.bind_group, &[]);
                    pass.draw(0..6, next..next + 1);
                }
                next += 1;
            }
            if banner_has {
                if let Some((tex, _, _)) = gpu.banner.as_ref() {
                    pass.set_bind_group(0, &tex.bind_group, &[]);
                    pass.draw(0..6, next..next + 1);
                }
            }
        }
        gpu.queue.submit(Some(enc.finish()));
        frame.present();
    }
}

fn pick_alpha(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    use wgpu::CompositeAlphaMode::*;
    for want in [PreMultiplied, PostMultiplied, Inherit] {
        if modes.contains(&want) {
            return want;
        }
    }
    modes.first().copied().unwrap_or(Opaque)
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> wgpu::BindGroup {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tex"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tex-bg"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

const SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@group(1) @binding(0) var<uniform> viewport: vec4<f32>;
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@vertex
fn vs(@builtin(vertex_index) vi: u32,
      @location(0) rect: vec4<f32>,
      @location(1) uvr: vec4<f32>,
      @location(2) color: vec4<f32>,
      @location(3) misc: vec4<f32>) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let c = corners[vi];
    // Rotate the quad around its own centre (misc.y = radians).
    let ang = misc.y;
    let ca = cos(ang);
    let sa = sin(ang);
    let local = (c - vec2<f32>(0.5, 0.5)) * rect.zw;
    let rot = vec2<f32>(local.x * ca - local.y * sa, local.x * sa + local.y * ca);
    let px = rect.xy + rect.zw * 0.5 + rot;
    let ndc = vec2<f32>(px.x / viewport.x * 2.0 - 1.0, 1.0 - px.y / viewport.y * 2.0);
    var u = uvr.x + uvr.z * c.x;
    if (misc.x > 0.5) { u = uvr.x + uvr.z * (1.0 - c.x); }
    let v = uvr.y + uvr.w * c.y;
    var o: VsOut;
    o.pos = vec4<f32>(ndc, 0.0, 1.0);
    o.uv = vec2<f32>(u, v);
    o.color = color;
    return o;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    let col = t * in.color;
    return vec4<f32>(col.rgb * col.a, col.a);
}
"#;
