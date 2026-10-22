//! wgpu viewport renderer. Draws into an offscreen multisampled target which
//! egui then displays as an image, so the 3D view has its own depth buffer and
//! anti-aliasing independent of the UI.
//!
//! Display meshes are uploaded once per body and re-uploaded only when the
//! kernel produces a new tessellation (pointer change of the `Arc<BodyMesh>`).

use bytemuck::{Pod, Zeroable};
use calibro_core::BodyId;
use calibro_kernel::BodyMesh;
use egui_wgpu::wgpu;
use std::collections::BTreeMap;
use std::sync::Arc;

pub const SAMPLES: u32 = 4;
/// Plain (non-sRGB) target: shaders write gamma-encoded colour, which is what
/// egui expects when it samples a user texture. This also avoids sRGB view
/// reinterpretation, which WebGL2 does not support.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Globals {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub light: [f32; 4],
    pub clip: [f32; 4],
    pub flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ObjectParams {
    pub color: [f32; 4],
    pub highlight: [f32; 4],
    pub ids: [i32; 4],
    pub material: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshVertex {
    pos: [f32; 3],
    nor: [f32; 3],
    face: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

struct GpuBody {
    source: Arc<BodyMesh>,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    edges: wgpu::Buffer,
    edge_count: u32,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// What to draw this frame for one body.
pub struct BodyDraw {
    pub id: BodyId,
    pub mesh: Arc<BodyMesh>,
    pub params: ObjectParams,
    pub show_faces: bool,
    pub show_edges: bool,
    pub xray: bool,
    pub edge_color: [f32; 4],
}

pub struct FrameInput {
    pub globals: Globals,
    pub bodies: Vec<BodyDraw>,
    /// Lines drawn with depth test (grid, axes, sketch on plane).
    pub lines: Vec<LineVertex>,
    /// Lines drawn on top of everything (selection, handles).
    pub overlay_lines: Vec<LineVertex>,
    /// Translucent triangles (sketch region fills, previews).
    pub fills: Vec<LineVertex>,
}

pub struct Targets {
    pub size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    pub resolve_tex: wgpu::Texture,
    pub resolve: wgpu::TextureView,
}

pub struct ViewportRenderer {
    mesh_opaque: wgpu::RenderPipeline,
    mesh_xray: wgpu::RenderPipeline,
    lines_depth: wgpu::RenderPipeline,
    lines_top: wgpu::RenderPipeline,
    fills: wgpu::RenderPipeline,
    background: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
    neutral_object: wgpu::BindGroup,
    bodies: BTreeMap<BodyId, GpuBody>,
    pub targets: Option<Targets>,
    pub texture_id: Option<egui::TextureId>,
}

fn uniform_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn buffer_init(
    device: &wgpu::Device,
    label: &str,
    data: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    // Avoid zero-sized buffers.
    let size = (data.len().max(16) as u64 + 3) & !3;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: true,
    });
    {
        let mut view = buf
            .slice(..)
            .get_mapped_range_mut()
            .expect("buffer mapped at creation");
        view.slice(..data.len()).copy_from_slice(data);
    }
    buf.unmap();
    buf
}

impl ViewportRenderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("calibro_viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });
        let globals_layout = uniform_layout(device, "globals_layout");
        let object_layout = uniform_layout(device, "object_layout");
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport_layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&object_layout)],
            immediate_size: 0,
        });

        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &mesh_attrs,
        };
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };
        let alpha_blend = wgpu::BlendState::ALPHA_BLENDING;

        let make = |label: &str,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    depth_write: bool,
                    compare: wgpu::CompareFunction,
                    bias: wgpu::DepthBiasState,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers,
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Faces are pushed slightly back so edges drawn at the same depth stay visible.
        let face_bias = wgpu::DepthBiasState {
            constant: 2,
            slope_scale: 1.0,
            clamp: 0.0,
        };
        let no_bias = wgpu::DepthBiasState::default();
        use wgpu::CompareFunction as C;
        use wgpu::PrimitiveTopology as T;
        let mesh_opaque = make(
            "mesh_opaque",
            "vs_mesh",
            "fs_mesh",
            &[Some(mesh_layout.clone())],
            T::TriangleList,
            true,
            C::Less,
            face_bias,
            None,
        );
        let mesh_xray = make(
            "mesh_xray",
            "vs_mesh",
            "fs_mesh",
            &[Some(mesh_layout)],
            T::TriangleList,
            false,
            C::Less,
            face_bias,
            Some(alpha_blend),
        );
        let lines_depth = make(
            "lines_depth",
            "vs_line",
            "fs_line",
            &[Some(line_layout.clone())],
            T::LineList,
            true,
            C::LessEqual,
            no_bias,
            Some(alpha_blend),
        );
        let lines_top = make(
            "lines_top",
            "vs_line",
            "fs_line",
            &[Some(line_layout.clone())],
            T::LineList,
            false,
            C::Always,
            no_bias,
            Some(alpha_blend),
        );
        let fills = make(
            "fills",
            "vs_line",
            "fs_line",
            &[Some(line_layout)],
            T::TriangleList,
            false,
            C::LessEqual,
            no_bias,
            Some(alpha_blend),
        );
        let background = make(
            "background",
            "vs_bg",
            "fs_bg",
            &[],
            T::TriangleList,
            false,
            C::Always,
            no_bias,
            None,
        );

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals_bg"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let neutral = ObjectParams {
            color: [1.0; 4],
            highlight: [0.0; 4],
            ids: [-1, -1, 0, 0],
            material: [0.0, 0.5, 0.0, 0.0],
        };
        let neutral_buf = buffer_init(
            device,
            "neutral_object",
            bytemuck::bytes_of(&neutral),
            wgpu::BufferUsages::UNIFORM,
        );
        let neutral_object = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("neutral_object_bg"),
            layout: &object_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: neutral_buf.as_entire_binding(),
            }],
        });
        Self {
            mesh_opaque,
            mesh_xray,
            lines_depth,
            lines_top,
            fills,
            background,
            globals_buf,
            globals_bg,
            object_layout,
            neutral_object,
            bodies: BTreeMap::new(),
            targets: None,
            texture_id: None,
        }
    }

    /// (Re)creates render targets when the viewport size changes. Returns true if recreated.
    pub fn ensure_targets(&mut self, device: &wgpu::Device, size: [u32; 2]) -> bool {
        // WebGL2 devices are limited to 2048 px textures; never exceed the device limit
        // (the image is then scaled up slightly instead of failing).
        let max = device.limits().max_texture_dimension_2d.clamp(1, 8192);
        let size = [size[0].clamp(1, max), size[1].clamp(1, max)];
        if self
            .targets
            .as_ref()
            .map(|t| t.size == size)
            .unwrap_or(false)
        {
            return false;
        }
        let tex = |label: &str, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = tex(
            "vp_msaa",
            COLOR_FORMAT,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth = tex(
            "vp_depth",
            DEPTH_FORMAT,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let resolve_tex = tex(
            "vp_resolve",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        self.targets = Some(Targets {
            size,
            msaa: msaa.create_view(&Default::default()),
            depth: depth.create_view(&Default::default()),
            resolve: resolve_tex.create_view(&Default::default()),
            resolve_tex,
        });
        true
    }

    fn sync_body(&mut self, device: &wgpu::Device, d: &BodyDraw) {
        let up_to_date = self
            .bodies
            .get(&d.id)
            .map(|g| Arc::ptr_eq(&g.source, &d.mesh))
            .unwrap_or(false);
        if up_to_date {
            return;
        }
        let m = &d.mesh;
        let verts: Vec<MeshVertex> = (0..m.positions.len())
            .map(|i| MeshVertex {
                pos: m.positions[i],
                nor: m.normals[i],
                face: m.face_ids[i],
            })
            .collect();
        let mut edge_verts = Vec::new();
        for pl in &m.edges {
            for w in pl.windows(2) {
                edge_verts.push(LineVertex {
                    pos: w[0],
                    color: d.edge_color,
                });
                edge_verts.push(LineVertex {
                    pos: w[1],
                    color: d.edge_color,
                });
            }
        }
        let uniform = buffer_init(
            device,
            "body_params",
            bytemuck::bytes_of(&d.params),
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("body_bg"),
            layout: &self.object_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let g = GpuBody {
            source: d.mesh.clone(),
            vbuf: buffer_init(
                device,
                "body_vb",
                bytemuck::cast_slice(&verts),
                wgpu::BufferUsages::VERTEX,
            ),
            ibuf: buffer_init(
                device,
                "body_ib",
                bytemuck::cast_slice(&m.indices),
                wgpu::BufferUsages::INDEX,
            ),
            index_count: m.indices.len() as u32,
            edges: buffer_init(
                device,
                "body_edges",
                bytemuck::cast_slice(&edge_verts),
                wgpu::BufferUsages::VERTEX,
            ),
            edge_count: edge_verts.len() as u32,
            uniform,
            bind_group,
        };
        self.bodies.insert(d.id, g);
    }

    /// Renders a frame into the offscreen target.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &FrameInput) {
        let Some(_) = &self.targets else { return };
        // upload / drop body buffers
        for d in &frame.bodies {
            self.sync_body(device, d);
        }
        let live: Vec<BodyId> = frame.bodies.iter().map(|b| b.id).collect();
        self.bodies.retain(|k, _| live.contains(k));
        for d in &frame.bodies {
            if let Some(g) = self.bodies.get(&d.id) {
                queue.write_buffer(&g.uniform, 0, bytemuck::bytes_of(&d.params));
            }
        }
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&frame.globals));

        let lines = (!frame.lines.is_empty()).then(|| {
            buffer_init(
                device,
                "lines",
                bytemuck::cast_slice(&frame.lines),
                wgpu::BufferUsages::VERTEX,
            )
        });
        let overlay = (!frame.overlay_lines.is_empty()).then(|| {
            buffer_init(
                device,
                "overlay",
                bytemuck::cast_slice(&frame.overlay_lines),
                wgpu::BufferUsages::VERTEX,
            )
        });
        let fills = (!frame.fills.is_empty()).then(|| {
            buffer_init(
                device,
                "fills",
                bytemuck::cast_slice(&frame.fills),
                wgpu::BufferUsages::VERTEX,
            )
        });

        let t = self.targets.as_ref().unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("viewport"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.msaa,
                    resolve_target: Some(&t.resolve),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Discard,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_bind_group(1, &self.neutral_object, &[]);
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);

            // opaque bodies
            pass.set_pipeline(&self.mesh_opaque);
            for d in frame
                .bodies
                .iter()
                .filter(|d| d.show_faces && !d.xray && d.params.color[3] >= 0.999)
            {
                let g = &self.bodies[&d.id];
                pass.set_bind_group(1, &g.bind_group, &[]);
                pass.set_vertex_buffer(0, g.vbuf.slice(..));
                pass.set_index_buffer(g.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..g.index_count, 0, 0..1);
            }
            // grid, axes and edges with depth test
            pass.set_bind_group(1, &self.neutral_object, &[]);
            pass.set_pipeline(&self.lines_depth);
            if let Some(b) = &lines {
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..frame.lines.len() as u32, 0..1);
            }
            for d in frame.bodies.iter().filter(|d| d.show_edges) {
                let g = &self.bodies[&d.id];
                if g.edge_count > 0 {
                    pass.set_vertex_buffer(0, g.edges.slice(..));
                    pass.draw(0..g.edge_count, 0..1);
                }
            }
            // translucent bodies (x-ray, glass)
            pass.set_pipeline(&self.mesh_xray);
            for d in frame
                .bodies
                .iter()
                .filter(|d| d.show_faces && (d.xray || d.params.color[3] < 0.999))
            {
                let g = &self.bodies[&d.id];
                pass.set_bind_group(1, &g.bind_group, &[]);
                pass.set_vertex_buffer(0, g.vbuf.slice(..));
                pass.set_index_buffer(g.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..g.index_count, 0, 0..1);
            }
            pass.set_bind_group(1, &self.neutral_object, &[]);
            if let Some(b) = &fills {
                pass.set_pipeline(&self.fills);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..frame.fills.len() as u32, 0..1);
            }
            if let Some(b) = &overlay {
                pass.set_pipeline(&self.lines_top);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..frame.overlay_lines.len() as u32, 0..1);
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Copies the last rendered frame into CPU memory as tightly packed RGBA8.
    /// Blocks until the GPU is done (native) — used for "Save image".
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_pixels(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let t = self.targets.as_ref()?;
        let (w, h) = (t.size[0], t.size[1]);
        let bytes_per_row = (w * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (bytes_per_row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.resolve_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let start = (row * bytes_per_row) as usize;
            out.extend_from_slice(&data[start..start + (w * 4) as usize]);
        }
        drop(data);
        buf.unmap();
        Some((w, h, out))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    //! Headless rendering tests. They need a GPU adapter (a software one such as
    //! Mesa lavapipe/llvmpipe is fine). If none is available they are skipped.
    use super::*;
    use crate::camera::CameraView;
    use calibro_core::CameraState;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn raw() -> RawWaker {
            fn c(_: *const ()) -> RawWaker {
                raw()
            }
            fn n(_: *const ()) {}
            static VT: RawWakerVTable = RawWakerVTable::new(c, n, n, n);
            RawWaker::new(std::ptr::null(), &VT)
        }
        let waker = unsafe { Waker::from_raw(raw()) };
        let mut cx = Context::from_waker(&waker);
        let mut f = std::pin::pin!(f);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    fn render_box(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color: [f32; 4],
        metallic: f32,
        roughness: f32,
    ) -> (u32, u32, Vec<u8>) {
        let solid = calibro_kernel::make_box([-10.0, -10.0, 0.0], [20.0, 20.0, 10.0]).unwrap();
        let mesh = Arc::new(calibro_kernel::tessellate(&solid, 0.01));
        let cam = CameraState {
            target: [0.0, 0.0, 5.0],
            distance: 60.0,
            pitch: 1.2, // looking mostly down at the top face
            ..Default::default()
        };
        let view = CameraView::new(&cam, glam::Vec2::new(128.0, 128.0));
        let mut r = ViewportRenderer::new(device);
        r.ensure_targets(device, [128, 128]);
        let frame = FrameInput {
            globals: Globals {
                view_proj: view.view_proj_f32().to_cols_array_2d(),
                eye: [view.eye.x as f32, view.eye.y as f32, view.eye.z as f32, 1.0],
                light: [0.3, -0.4, 0.86, 0.0],
                clip: [0.0; 4],
                flags: [0.0, 128.0, 0.0, 0.0],
            },
            bodies: vec![BodyDraw {
                id: BodyId {
                    feature: 1,
                    index: 0,
                },
                mesh,
                params: ObjectParams {
                    color,
                    highlight: [0.0; 4],
                    ids: [-1, -1, 0, 0],
                    material: [metallic, roughness, 0.0, 0.0],
                },
                show_faces: true,
                show_edges: true,
                xray: false,
                edge_color: [0.0, 0.0, 0.0, 1.0],
            }],
            lines: vec![],
            overlay_lines: vec![],
            fills: vec![],
        };
        r.render(device, queue, &frame);
        r.read_pixels(device, queue).unwrap()
    }

    fn px(img: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.0 + x) * 4) as usize;
        [img.2[i], img.2[i + 1], img.2[i + 2], img.2[i + 3]]
    }

    #[test]
    fn renders_box_over_background() {
        let Some((d, q)) = device() else {
            eprintln!("no GPU adapter available: skipping rendering test");
            return;
        };
        let img = render_box(&d, &q, [0.72, 0.74, 0.77, 1.0], 0.0, 0.55);
        let center = px(&img, 64, 64);
        let corner = px(&img, 2, 2);
        eprintln!("matte centre {center:?} background corner {corner:?}");
        assert_ne!(center, corner, "the box must cover the centre of the image");
        // background top is a light grey-blue gradient
        assert!(corner[0] > 180 && corner[2] > 180, "{corner:?}");
        // the top face of a light grey box is clearly lit
        assert!(center[0] > 120, "top face too dark: {center:?}");
    }

    #[test]
    #[ignore = "diagnostic only"]
    fn debug_metal_angles() {
        let Some((d, q)) = device() else { return };
        for (pitch, dist) in [(1.2, 60.0), (0.5, 60.0), (0.5, 230.0)] {
            let solid = calibro_kernel::make_box([-10.0, -10.0, 0.0], [20.0, 20.0, 10.0]).unwrap();
            let mesh = Arc::new(calibro_kernel::tessellate(&solid, 0.01));
            let cam = CameraState {
                target: [0.0, 0.0, 5.0],
                distance: dist,
                pitch,
                yaw: -1.05,
                ..Default::default()
            };
            let view = CameraView::new(&cam, glam::Vec2::new(128.0, 128.0));
            let mut r = ViewportRenderer::new(&d);
            r.ensure_targets(&d, [128, 128]);
            let frame = FrameInput {
                globals: Globals {
                    view_proj: view.view_proj_f32().to_cols_array_2d(),
                    eye: [view.eye.x as f32, view.eye.y as f32, view.eye.z as f32, 1.0],
                    light: [0.3, -0.4, 0.86, 0.0],
                    clip: [0.0; 4],
                    flags: [0.0, 128.0, 0.0, 0.0],
                },
                bodies: vec![BodyDraw {
                    id: BodyId {
                        feature: 1,
                        index: 0,
                    },
                    mesh,
                    params: ObjectParams {
                        color: [0.91, 0.92, 0.92, 1.0],
                        highlight: [0.0; 4],
                        ids: [-1, -1, 0, 0],
                        material: [1.0, 0.3, 0.0, 0.0],
                    },
                    show_faces: true,
                    show_edges: false,
                    xray: false,
                    edge_color: [0.0; 4],
                }],
                lines: vec![],
                overlay_lines: vec![],
                fills: vec![],
            };
            r.render(&d, &q, &frame);
            let img = r.read_pixels(&d, &q).unwrap();
            // find a pixel on the top face: project (0,0,10)
            let p = view.project(glam::DVec3::new(0.0, 0.0, 10.0)).unwrap();
            eprintln!(
                "pitch {pitch} dist {dist} eye {:?} top {:?}",
                view.eye,
                px(&img, p.x as u32, p.y as u32)
            );
        }
    }

    #[test]
    fn metallic_top_face_is_not_black() {
        let Some((d, q)) = device() else {
            return;
        };
        let img = render_box(&d, &q, [0.91, 0.92, 0.92, 1.0], 1.0, 0.3);
        let center = px(&img, 64, 64);
        eprintln!("metal centre {center:?}");
        assert!(center[0] > 100, "metallic top face too dark: {center:?}");
    }
}
