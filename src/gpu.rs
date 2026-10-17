//! wgpu fractal pipeline, embedded in egui through a `PaintCallback`.

use eframe::egui_wgpu::{self, wgpu};
use eframe::egui;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub center: [f32; 2],
    pub resolution: [f32; 2],
    pub julia_c: [f32; 2],
    pub scale: f32,
    pub power: f32,
    pub max_iter: u32,
    pub kind: u32,
    pub palette: u32,
    pub ssaa: u32,
    pub color_scale: f32,
    pub color_offset: f32,
    pub linear_out: u32,
    pub orbit_len: u32,
}

/// Maximum number of reference-orbit points.
pub const ORBIT_CAP: usize = 20_002;

struct FractalGpu {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    orbit_buffer: wgpu::Buffer,
}

/// Creates the pipeline and stores it in the egui renderer's shared resources.
pub fn init(render_state: &egui_wgpu::RenderState) {
    let device = &render_state.device;

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fractal_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("fractal.wgsl").into()),
    });

    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fractal_uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let orbit_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fractal_orbit"),
        size: (ORBIT_CAP * std::mem::size_of::<[f32; 2]>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fractal_bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }, wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fractal_bg"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: orbit_buffer.as_entire_binding() },
        ],
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fractal_pl"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fractal_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            targets: &[Some(render_state.target_format.into())],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    render_state
        .renderer
        .write()
        .callback_resources
        .insert(FractalGpu { pipeline, bind_group, uniform_buffer, orbit_buffer });
}

struct FractalCallback {
    uniforms: Uniforms,
    orbit: Vec<[f32; 2]>,
}

impl egui_wgpu::CallbackTrait for FractalCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get::<FractalGpu>() {
            queue.write_buffer(&gpu.uniform_buffer, 0, bytemuck::bytes_of(&self.uniforms));
            if !self.orbit.is_empty() {
                queue.write_buffer(&gpu.orbit_buffer, 0, bytemuck::cast_slice(&self.orbit));
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(gpu) = resources.get::<FractalGpu>() {
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

pub fn paint_callback(rect: egui::Rect, uniforms: Uniforms, orbit: Vec<[f32; 2]>) -> egui::PaintCallback {
    egui_wgpu::Callback::new_paint_callback(rect, FractalCallback { uniforms, orbit })
}
