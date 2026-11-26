//! GPU state: surface, depth buffer, procedural Roblox-like sky, lit part meshes and the egui renderer.
//! Errors are plain strings logged by the caller.
use crate::mesh;
use moge_math::OrbitCamera;
use moge_scene::{Lighting, Scene};
use std::ops::Range;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const INSTANCE_FLOATS: usize = 24;
const GLOBAL_FLOATS: usize = 16 + 9 * 4;

pub struct SceneInput<'a> {
    pub cam: &'a OrbitCamera,
    pub lighting: &'a Lighting,
    /// Viewport in physical pixels: x, y, w, h.
    pub viewport: [f32; 4],
    pub draw3d: bool,
}

pub struct EguiInput {
    pub prims: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub ppp: f32,
}

pub struct Gfx {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    srgb: bool,
    sky_pipe: wgpu::RenderPipeline,
    opaque_pipe: wgpu::RenderPipeline,
    alpha_pipe: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    mesh_buf: wgpu::Buffer,
    shape_ranges: [Range<u32>; mesh::SHAPES],
    instances: Option<wgpu::Buffer>,
    /// (shape, instance range) for opaque and transparent batches.
    opaque: Vec<(usize, Range<u32>)>,
    alpha: Vec<(usize, Range<u32>)>,
    depth_view: wgpu::TextureView,
    egui: egui_wgpu::Renderer,
}

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_ne_bytes()).collect()
}

fn create_depth_view(device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Gfx {
    pub async fn new(window: Arc<Window>) -> Result<Gfx, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN | wgpu::Backends::GL, ..Default::default() });
        let surface = instance.create_surface(window.clone()).map_err(|e| format!("Failed to create the render surface: {e}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| "No compatible GPU adapter was found".to_string())?;
        log::info!("GPU adapter: {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("moge-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("Failed to create the GPU device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| f.is_srgb()).or_else(|| caps.formats.first().copied()).ok_or_else(|| "The surface reports no supported formats".to_string())?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("scene"), source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor { label: Some("globals"), size: (GLOBAL_FLOATS * 4) as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("globals"), layout: &bgl, entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }] });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("scene"), bind_group_layouts: &[&bgl], push_constant_ranges: &[] });

        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let inst_attrs = wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4];
        let part_buffers = [
            wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &mesh_attrs },
            wgpu::VertexBufferLayout { array_stride: (INSTANCE_FLOATS * 4) as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst_attrs },
        ];
        let make = |label: &str, vs: &str, fs: &str, buffers: &[wgpu::VertexBufferLayout], depth_write: bool, depth_always: bool, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: vs, compilation_options: wgpu::PipelineCompilationOptions::default(), buffers },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: fs,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth_write,
                    depth_compare: if depth_always { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::Less },
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let sky_pipe = make("sky", "vs_sky", "fs_sky", &[], false, true, None);
        let opaque_pipe = make("parts", "vs_part", "fs_part", &part_buffers, true, false, Some(wgpu::BlendState::REPLACE));
        let alpha_pipe = make("parts-alpha", "vs_part", "fs_part", &part_buffers, false, false, Some(wgpu::BlendState::ALPHA_BLENDING));

        let (verts, shape_ranges) = mesh::build();
        let mesh_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("meshes"), contents: &bytes(&verts), usage: wgpu::BufferUsages::VERTEX });
        let depth_view = create_depth_view(&device, config.width, config.height);
        let egui = egui_wgpu::Renderer::new(&device, format, None, 1, false);
        Ok(Gfx {
            window,
            surface,
            device,
            queue,
            srgb: format.is_srgb(),
            config,
            sky_pipe,
            opaque_pipe,
            alpha_pipe,
            bind_group,
            globals,
            mesh_buf,
            shape_ranges,
            instances: None,
            opaque: Vec::new(),
            alpha: Vec::new(),
            depth_view,
            egui,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, w, h);
    }

    /// Uploads the parts of `scene` as instance data (call when the scene changed).
    pub fn upload_scene(&mut self, scene: &Scene) {
        let mut groups: Vec<Vec<f32>> = vec![Vec::new(); mesh::SHAPES * 2];
        for p in &scene.parts {
            if p.alpha <= 0.01 {
                continue;
            }
            let g = mesh::shape_index(p.shape) * 2 + usize::from(p.alpha < 0.99);
            let v = &mut groups[g];
            v.extend_from_slice(&p.model);
            v.extend_from_slice(&[p.color[0], p.color[1], p.color[2], p.alpha]);
            v.extend_from_slice(&[if p.material == 288 { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]);
        }
        let mut all: Vec<f32> = Vec::new();
        self.opaque.clear();
        self.alpha.clear();
        for (i, g) in groups.iter().enumerate() {
            if g.is_empty() {
                continue;
            }
            let start = (all.len() / INSTANCE_FLOATS) as u32;
            all.extend_from_slice(g);
            let range = start..(all.len() / INSTANCE_FLOATS) as u32;
            let list = if i % 2 == 0 { &mut self.opaque } else { &mut self.alpha };
            list.push((i / 2, range));
        }
        self.instances = if all.is_empty() {
            None
        } else {
            Some(self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("instances"), contents: &bytes(&all), usage: wgpu::BufferUsages::VERTEX }))
        };
    }

    fn write_globals(&self, s: &SceneInput) {
        let aspect = (s.viewport[2] / s.viewport[3].max(1.0)).max(0.01);
        let vp = s.cam.view_proj(aspect);
        let l = s.lighting;
        let sun = moge_scene::sun_direction(l.sky.clock_time);
        let fwd = s.cam.forward();
        let (right, up) = s.cam.basis();
        let t = 30f32.to_radians().tan();
        let eye = s.cam.eye();
        let mut g: Vec<f32> = Vec::with_capacity(GLOBAL_FLOATS);
        g.extend_from_slice(&vp);
        g.extend_from_slice(&[sun[0], sun[1], sun[2], l.brightness]);
        g.extend_from_slice(&[l.ambient[0], l.ambient[1], l.ambient[2], l.fog_start]);
        g.extend_from_slice(&[l.outdoor_ambient[0], l.outdoor_ambient[1], l.outdoor_ambient[2], l.fog_end]);
        g.extend_from_slice(&[l.fog_color[0], l.fog_color[1], l.fog_color[2], if self.srgb { 1.0 } else { 0.0 }]);
        g.extend_from_slice(&[eye[0], eye[1], eye[2], 0.0]);
        g.extend_from_slice(&[fwd[0], fwd[1], fwd[2], 0.0]);
        g.extend_from_slice(&[right[0] * t * aspect, right[1] * t * aspect, right[2] * t * aspect, 0.0]);
        g.extend_from_slice(&[up[0] * t, up[1] * t, up[2] * t, 0.0]);
        g.extend_from_slice(&[(l.sky.star_count as f32 / 100_000.0).clamp(0.0, 0.05), l.sky.sun_size, l.sky.moon_size, if l.sky.celestial_bodies { 1.0 } else { 0.0 }]);
        self.queue.write_buffer(&self.globals, 0, &bytes(&g));
    }

    pub fn render(&mut self, s: &SceneInput, ui: EguiInput) {
        let (sw, sh) = (self.config.width as f32, self.config.height as f32);
        let x = s.viewport[0].clamp(0.0, sw - 1.0).floor();
        let y = s.viewport[1].clamp(0.0, sh - 1.0).floor();
        let w = s.viewport[2].min(sw - x).floor();
        let h = s.viewport[3].min(sh - y).floor();
        let draw3d = s.draw3d && w >= 2.0 && h >= 2.0;
        if draw3d {
            self.write_globals(&SceneInput { cam: s.cam, lighting: s.lighting, viewport: [x, y, w, h], draw3d: true });
        }
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("Surface frame unavailable ({e}); reconfiguring");
                self.surface.configure(&self.device, &self.config);
                for id in &ui.textures.free {
                    self.egui.free_texture(id);
                }
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        for (id, delta) in &ui.textures.set {
            self.egui.update_texture(&self.device, &self.queue, *id, delta);
        }
        let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.config.width, self.config.height], pixels_per_point: ui.ppp };
        let extra = self.egui.update_buffers(&self.device, &self.queue, &mut enc, &ui.prims, &sd);
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.07, g: 0.07, b: 0.08, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if draw3d {
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
                pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_pipeline(&self.sky_pipe);
                pass.draw(0..3, 0..1);
                if let Some(inst) = &self.instances {
                    pass.set_vertex_buffer(0, self.mesh_buf.slice(..));
                    pass.set_vertex_buffer(1, inst.slice(..));
                    pass.set_pipeline(&self.opaque_pipe);
                    for (shape, r) in &self.opaque {
                        pass.draw(self.shape_ranges[*shape].clone(), r.clone());
                    }
                    pass.set_pipeline(&self.alpha_pipe);
                    for (shape, r) in &self.alpha {
                        pass.draw(self.shape_ranges[*shape].clone(), r.clone());
                    }
                }
            }
        }
        {
            let mut pass = enc
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.egui.render(&mut pass, &ui.prims, &sd);
        }
        self.queue.submit(extra.into_iter().chain(std::iter::once(enc.finish())));
        frame.present();
        for id in &ui.textures.free {
            self.egui.free_texture(id);
        }
    }
}
