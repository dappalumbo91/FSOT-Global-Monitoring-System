//! wgpu globe. The scene is drawn into an offscreen target, egui is composited
//! on top, and that target is blitted to the swapchain. A screenshot copies
//! the same target, so the saved frame matches the window.

use crate::camera::Camera;
use crate::layers::FrameSettings;
use crate::mesh::{self, LineVertex, Vertex};
use glam::Vec3;
use image::RgbaImage;
use std::path::PathBuf;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.004,
    g: 0.005,
    b: 0.010,
    a: 1.0,
};

const ADDITIVE: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    sun_dir: [f32; 4],
    params: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<Globals>() == 112);

struct GpuMesh {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    indices: u32,
}

pub struct GlobeRenderer {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    day_view: wgpu::TextureView,
    night_view: wgpu::TextureView,
    elev_view: wgpu::TextureView,
    globe_bg: wgpu::BindGroup,
    globe_pipeline: wgpu::RenderPipeline,
    star_pipeline: wgpu::RenderPipeline,
    atmo_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    marker_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    sphere: GpuMesh,
    atmosphere: GpuMesh,
    stars: GpuMesh,
    grat_major: GpuMesh,
    grat_minor: GpuMesh,
    faults: Option<GpuMesh>,
    volcanoes: Option<GpuMesh>,
    quakes: Option<GpuMesh>,
    ocean: Option<GpuMesh>,
    weather: Option<GpuMesh>,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    blit_layout: wgpu::BindGroupLayout,
    blit_bg: wgpu::BindGroup,
    egui: egui_wgpu::Renderer,
}

pub enum OverlayKind {
    Faults,
    Volcanoes,
    Quakes,
    Ocean,
    Weather,
}

pub enum FrameResult {
    Drawn(Option<RgbaImage>),
    Retry,
    Lost,
}

impl GlobeRenderer {
    pub fn new(window: &Arc<Window>) -> Result<Self, String> {
        let day = load_equirect(project_path("assets/earth_day.jpg"))?;
        let night = load_equirect(project_path("assets/earth_night.png"))?;
        log::info!(
            "imagery {}x{} day, {}x{} night",
            day.width(),
            day.height(),
            night.width(),
            night.height()
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("create surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        log::info!("GPU {} ({:?})", info.name, info.backend);

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fsot-device"),
            ..Default::default()
        }))
        .map_err(|e| format!("request device: {e}"))?;

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| "surface has no default configuration".to_string())?;
        config.format = prefer_unorm(&caps, config.format);
        config.present_mode = wgpu::PresentMode::Fifo;
        config.width = width;
        config.height = height;
        surface.configure(&device, &config);

        let globe_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("globe"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/globe.wgsl").into()),
        });
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sky.wgsl").into()),
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blit.wgsl").into()),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("earth-textures"),
            entries: &[
                texture_entry(0, true),
                texture_entry(1, true),
                sampler_entry(2),
                texture_entry(3, true),
            ],
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[texture_entry(0, true), sampler_entry(1)],
        });
        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let globe_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("globe-layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit-layout"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });

        let surface_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let surface_layout = surface_layout(&surface_attrs);
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };

        let globe_pipeline = pipeline(
            &device, "globe", &globe_layout, &globe_shader, "vs_globe", "fs_globe",
            &[Some(surface_layout.clone())], wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back), None, true, wgpu::CompareFunction::Less, 0,
        );
        let star_pipeline = pipeline(
            &device, "stars", &scene_layout, &sky_shader, "vs_surface", "fs_star",
            &[Some(surface_layout.clone())], wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Front), Some(ADDITIVE), false, wgpu::CompareFunction::Always, 0,
        );
        let atmo_pipeline = pipeline(
            &device, "atmosphere", &scene_layout, &sky_shader, "vs_surface", "fs_atmo",
            &[Some(surface_layout)], wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back), Some(ADDITIVE), false, wgpu::CompareFunction::Less, 0,
        );
        let marker_layout = line_layout.clone();
        let line_pipeline = pipeline(
            &device, "graticule", &scene_layout, &sky_shader, "vs_line", "fs_line",
            &[Some(line_layout)], wgpu::PrimitiveTopology::LineList,
            None, Some(wgpu::BlendState::ALPHA_BLENDING), false, wgpu::CompareFunction::LessEqual, 0,
        );
        let marker_pipeline = pipeline(
            &device, "markers", &scene_layout, &sky_shader, "vs_line", "fs_line",
            &[Some(marker_layout)], wgpu::PrimitiveTopology::TriangleList,
            None, Some(wgpu::BlendState::ALPHA_BLENDING), true, wgpu::CompareFunction::Less, 0,
        );
        let blit_pipeline = blit_pipeline(&device, &blit_pl, &blit_shader, config.format);

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals-bg"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("earth"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let day_view = upload_mips(&device, &queue, "day", &rgba_mips(day));
        let night_view = upload_mips(&device, &queue, "night", &rgba_mips(night));
        let elev_view = upload_linear(&device, &queue, "elevation", 1, 1, &[0, 0, 0, 0]);
        let globe_bg = earth_bind_group(&device, &tex_layout, &day_view, &night_view, &elev_view, &sampler);

        let earth = mesh::uv_sphere(160, 320, 1.0);
        let sphere = upload(&device, "sphere", &earth.vertices, &earth.indices);
        let shell = mesh::uv_sphere(64, 128, 1.028);
        let atmosphere = upload(&device, "atmosphere", &shell.vertices, &shell.indices);
        let sky = mesh::uv_sphere(48, 96, 90.0);
        let stars = upload(&device, "stars", &sky.vertices, &sky.indices);
        let major = mesh::graticule(15.0, false);
        let grat_major = upload(&device, "grat-major", &major.vertices, &major.indices);
        let minor = mesh::graticule(5.0, true);
        let grat_minor = upload(&device, "grat-minor", &minor.vertices, &minor.indices);

        let (color, color_view, depth_view, blit_bg) = make_targets(&device, &blit_layout, width, height);
        let egui = egui_wgpu::Renderer::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                dithering: false,
                predictable_texture_filtering: false,
            },
        );

        Ok(Self {
            instance,
            surface,
            device,
            queue,
            config,
            width,
            height,
            globals,
            globals_bg,
            tex_layout,
            sampler,
            day_view,
            night_view,
            elev_view,
            globe_bg,
            globe_pipeline,
            star_pipeline,
            atmo_pipeline,
            line_pipeline,
            marker_pipeline,
            blit_pipeline,
            sphere,
            atmosphere,
            stars,
            grat_major,
            grat_minor,
            faults: None,
            volcanoes: None,
            quakes: None,
            ocean: None,
            weather: None,
            color,
            color_view,
            depth_view,
            blit_layout,
            blit_bg,
            egui,
        })
    }

    pub fn set_overlay(&mut self, kind: OverlayKind, mesh: &crate::mesh::Mesh<LineVertex>) {
        let uploaded = if mesh.indices.is_empty() {
            None
        } else {
            Some(upload(&self.device, "overlay", &mesh.vertices, &mesh.indices))
        };
        match kind {
            OverlayKind::Faults => self.faults = uploaded,
            OverlayKind::Volcanoes => self.volcanoes = uploaded,
            OverlayKind::Quakes => self.quakes = uploaded,
            OverlayKind::Ocean => self.ocean = uploaded,
            OverlayKind::Weather => self.weather = uploaded,
        }
    }

    /// Replace the land-tint texture. RGB is the hypsometric color and A is height.
    pub fn set_elevation(&mut self, width: u32, height: u32, rgba: &[u8]) {
        if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
            return;
        }
        let view = upload_linear(&self.device, &self.queue, "elevation", width, height, rgba);
        self.globe_bg = earth_bind_group(
            &self.device,
            &self.tex_layout,
            &self.day_view,
            &self.night_view,
            &view,
            &self.sampler,
        );
        self.elev_view = view;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.width && height == self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        let (color, color_view, depth_view, blit_bg) =
            make_targets(&self.device, &self.blit_layout, width, height);
        self.color = color;
        self.color_view = color_view;
        self.depth_view = depth_view;
        self.blit_bg = blit_bg;
    }

    pub fn recreate_surface(&mut self, window: &Arc<Window>) -> Result<(), String> {
        self.surface = self
            .instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        self.surface.configure(&self.device, &self.config);
        Ok(())
    }

    pub fn render(
        &mut self,
        camera: &Camera,
        sun: Vec3,
        settings: &FrameSettings,
        textures_delta: &egui::TexturesDelta,
        primitives: &[egui::ClippedPrimitive],
        screen: &egui_wgpu::ScreenDescriptor,
        capture: bool,
    ) -> Result<FrameResult, String> {
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.egui.update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        for id in &textures_delta.free {
            self.egui.free_texture(id);
        }

        let (pw, ph) = (screen.size_in_pixels[0], screen.size_in_pixels[1]);
        if pw != self.width || ph != self.height {
            self.resize(pw, ph);
        }
        if self.width == 0 || self.height == 0 {
            return Ok(FrameResult::Retry);
        }

        let (frame, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(FrameResult::Retry);
            }
            wgpu::CurrentSurfaceTexture::Lost => return Ok(FrameResult::Lost),
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return Ok(FrameResult::Retry),
        };

        let aspect = self.width as f32 / self.height as f32;
        let eye = camera.eye();
        let sun = if sun.length_squared() > 0.0 { sun.normalize() } else { Vec3::Y };
        let globals = Globals {
            view_proj: camera.view_proj(aspect).to_cols_array_2d(),
            camera_pos: [eye.x, eye.y, eye.z, 1.0],
            sun_dir: [sun.x, sun.y, sun.z, settings.city_lights],
            params: [
                if settings.sun { 1.0 } else { 0.0 },
                if settings.atmosphere { 1.0 } else { 0.0 },
                if settings.stars { 1.0 } else { 0.0 },
                if settings.elevation { 1.0 } else { 0.0 },
            ],
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let color_view = self.color_view.clone();
        let depth_view = self.depth_view.clone();
        let surface_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(color_attachment(&color_view, wgpu::LoadOp::Clear(CLEAR)))],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if settings.stars {
                draw_mesh(&mut pass, &self.star_pipeline, &self.globals_bg, None, &self.stars);
            }
            draw_mesh(
                &mut pass,
                &self.globe_pipeline,
                &self.globals_bg,
                Some(&self.globe_bg),
                &self.sphere,
            );
            if settings.faults {
                draw_optional(&mut pass, &self.line_pipeline, &self.globals_bg, self.faults.as_ref());
            }
            if settings.graticule {
                draw_mesh(&mut pass, &self.line_pipeline, &self.globals_bg, None, &self.grat_major);
            }
            if settings.minor_graticule {
                draw_mesh(&mut pass, &self.line_pipeline, &self.globals_bg, None, &self.grat_minor);
            }
            if settings.ocean {
                draw_optional(&mut pass, &self.marker_pipeline, &self.globals_bg, self.ocean.as_ref());
            }
            if settings.volcanoes {
                draw_optional(&mut pass, &self.marker_pipeline, &self.globals_bg, self.volcanoes.as_ref());
            }
            if settings.quakes {
                draw_optional(&mut pass, &self.marker_pipeline, &self.globals_bg, self.quakes.as_ref());
            }
            if settings.weather {
                draw_optional(&mut pass, &self.line_pipeline, &self.globals_bg, self.weather.as_ref());
            }
            if settings.atmosphere {
                draw_mesh(&mut pass, &self.atmo_pipeline, &self.globals_bg, None, &self.atmosphere);
            }
        }

        let extra = self.egui.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            primitives,
            screen,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(color_attachment(&color_view, wgpu::LoadOp::Load))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.egui.render(&mut pass.forget_lifetime(), primitives, screen);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(color_attachment(
                    &surface_view,
                    wgpu::LoadOp::Clear(CLEAR),
                ))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.blit_bg, &[]);
            pass.draw(0..3, 0..1);
        }

        let readback = if capture {
            Some(self.copy_color(&mut encoder))
        } else {
            None
        };

        self.queue.submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        self.queue.present(frame);
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
        }

        let image = if let Some((buffer, bpr)) = readback {
            Some(map_image(&self.device, &buffer, self.width, self.height, bpr)?)
        } else {
            None
        };
        Ok(FrameResult::Drawn(image))
    }

    fn copy_color(&self, encoder: &mut wgpu::CommandEncoder) -> (wgpu::Buffer, u32) {
        let bpr = padded_bpr(self.width);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: bpr as u64 * self.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        (buffer, bpr)
    }
}

fn draw_optional<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a wgpu::RenderPipeline,
    globals_bg: &'a wgpu::BindGroup,
    mesh: Option<&'a GpuMesh>,
) {
    if let Some(mesh) = mesh {
        draw_mesh(pass, pipeline, globals_bg, None, mesh);
    }
}

fn draw_mesh<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a wgpu::RenderPipeline,
    globals_bg: &'a wgpu::BindGroup,
    extra: Option<&'a wgpu::BindGroup>,
    mesh: &'a GpuMesh,
) {
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, globals_bg, &[]);
    if let Some(bg) = extra {
        pass.set_bind_group(1, bg, &[]);
    }
    pass.set_vertex_buffer(0, mesh.vb.slice(..));
    pass.set_index_buffer(mesh.ib.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..mesh.indices, 0, 0..1);
}

fn color_attachment(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    }
}

fn surface_layout(attrs: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: attrs,
    }
}

fn pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    topology: wgpu::PrimitiveTopology,
    cull: Option<wgpu::Face>,
    blend: Option<wgpu::BlendState>,
    depth_write: bool,
    depth_compare: wgpu::CompareFunction,
    bias: i32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs),
            buffers,
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: cull,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(depth_compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: bias,
                slope_scale: if bias != 0 { -1.0 } else { 0.0 },
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn blit_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("blit"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_blit"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_blit"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn make_targets(
    device: &wgpu::Device,
    blit_layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView, wgpu::BindGroup) {
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("blit"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let blit_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blit-bg"),
        layout: blit_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&blit_sampler),
            },
        ],
    });
    let _ = depth;
    (color, color_view, depth_view, blit_bg)
}

fn upload<V: bytemuck::Pod>(
    device: &wgpu::Device,
    label: &str,
    vertices: &[V],
    indices: &[u32],
) -> GpuMesh {
    let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    GpuMesh { vb, ib, indices: indices.len() as u32 }
}

fn earth_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    day: &wgpu::TextureView,
    night: &wgpu::TextureView,
    elev: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("earth-bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(day),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(night),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(elev),
            },
        ],
    })
}

fn upload_linear(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
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
        &rgba[..(width * height * 4) as usize],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn upload_mips(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, mips: &[RgbaImage]) -> wgpu::TextureView {
    let width = mips[0].width();
    let height = mips[0].height();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, image) in mips.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * image.width()),
                rows_per_image: Some(image.height()),
            },
            wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn prefer_unorm(caps: &wgpu::SurfaceCapabilities, fallback: wgpu::TextureFormat) -> wgpu::TextureFormat {
    let linear = match fallback {
        wgpu::TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8Unorm,
        other => other,
    };
    if caps.formats.contains(&linear) {
        linear
    } else if caps.formats.contains(&wgpu::TextureFormat::Bgra8Unorm) {
        wgpu::TextureFormat::Bgra8Unorm
    } else if caps.formats.contains(&wgpu::TextureFormat::Rgba8Unorm) {
        wgpu::TextureFormat::Rgba8Unorm
    } else {
        fallback
    }
}

fn padded_bpr(width: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let row = width * 4;
    row.div_ceil(align) * align
}

fn map_image(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    width: u32,
    height: u32,
    bpr: u32,
) -> Result<RgbaImage, String> {
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("poll readback: {e}"))?;
    let mapped = match rx.try_recv() {
        Ok(result) => result.map_err(|e| format!("map readback: {e}"))?,
        Err(_) => {
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            rx.recv_timeout(std::time::Duration::from_secs(3))
                .map_err(|_| "timed out mapping the frame".to_string())?
                .map_err(|e| format!("map readback: {e}"))?
        }
    };
    let _ = mapped;
    let view = slice
        .get_mapped_range()
        .map_err(|e| format!("mapped range: {e}"))?;
    let row = (width * 4) as usize;
    let stride = bpr as usize;
    let mut raw = vec![0u8; row * height as usize];
    for y in 0..height as usize {
        let src = y * stride;
        let dst = y * row;
        raw[dst..dst + row].copy_from_slice(&view[src..src + row]);
    }
    drop(view);
    buffer.unmap();
    RgbaImage::from_raw(width, height, raw).ok_or_else(|| "image size mismatch".to_string())
}

pub fn project_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn load_equirect(path: PathBuf) -> Result<RgbaImage, String> {
    let img = image::open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?
        .to_rgba8();
    if img.width() != img.height() * 2 {
        return Err(format!(
            "{} is {}x{}, expected a 2:1 equirectangular image",
            path.display(),
            img.width(),
            img.height()
        ));
    }
    Ok(img)
}

fn rgba_mips(base: RgbaImage) -> Vec<RgbaImage> {
    let levels = mip_level_count(base.width(), base.height()) as usize;
    let mut images = Vec::with_capacity(levels);
    images.push(base);
    while images.len() < levels {
        images.push(downsample(images.last().expect("mip")));
    }
    images
}

fn mip_level_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).leading_zeros()
}

fn downsample(src: &RgbaImage) -> RgbaImage {
    let sw = src.width() as usize;
    let sh = src.height() as usize;
    let dw = (sw / 2).max(1);
    let dh = (sh / 2).max(1);
    let s = src.as_raw();
    let mut raw = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        let y0 = (y * 2).min(sh - 1);
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..dw {
            let x0 = (x * 2).min(sw - 1);
            let x1 = (x0 + 1).min(sw - 1);
            let mut acc = [0u16; 4];
            for (yy, xx) in [(y0, x0), (y0, x1), (y1, x0), (y1, x1)] {
                let i = (yy * sw + xx) * 4;
                for c in 0..4 {
                    acc[c] += u16::from(s[i + c]);
                }
            }
            let o = (y * dw + x) * 4;
            for c in 0..4 {
                raw[o + c] = (acc[c] / 4) as u8;
            }
        }
    }
    RgbaImage::from_raw(dw as u32, dh as u32, raw).expect("downsampled image")
}
