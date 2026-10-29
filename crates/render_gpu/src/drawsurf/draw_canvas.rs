use std::num::NonZeroU64;

use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::mesh::VertexBufferLayout;
use bevy::prelude::*;
use bevy::render::Extract;
use bevy::render::RenderStartup;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
    BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites, CompareFunction, DepthBiasState,
    DepthStencilState, Extent3d, FragmentState, FrontFace, MultisampleState, Origin3d,
    PipelineCache, PrimitiveState, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, SpecializedRenderPipeline,
    SpecializedRenderPipelines, StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, TextureViewDescriptor, VertexAttribute, VertexFormat, VertexState,
    VertexStepMode,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shader::Shader;

use super::scene_depth::{SCENE_DEPTH_FORMAT, SceneDepthTexture};

const SHADER_PATH: &str = "embedded://render_gpu/drawsurf/draw_canvas.wgsl";
const EASEL_SHADER_PATH: &str = "embedded://render_gpu/drawsurf/draw_easel.wgsl";
const PARAMS_SIZE: u64 = 112;
const EASEL_VERTEX_CAPACITY: usize = 256;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DrawCanvasSet;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CanvasUniform {
    clip_from_world: [[f32; 4]; 4],
    origin: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
}

#[derive(Resource)]
struct DrawCanvasPipeline {
    shader: Handle<Shader>,
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    params: Buffer,
}

#[derive(Resource)]
struct EaselPipeline {
    shader: Handle<Shader>,
    layout: BindGroupLayoutDescriptor,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct EaselVertex {
    position: [f32; 3],
    color: [f32; 4],
}

#[derive(Resource)]
struct EaselGpu {
    buffer: Buffer,
    vertices: Vec<EaselVertex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct DrawCanvasKey {
    format: TextureFormat,
    overlay: bool,
}

impl SpecializedRenderPipeline for DrawCanvasPipeline {
    type Key = DrawCanvasKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("draw_mode_canvas".into()),
            layout: vec![self.layout.clone()],
            immediate_size: 0,
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: Vec::new(),
                constants: Vec::new(),
                entry_point: Some("vs_canvas".into()),
                buffers: Vec::new(),
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs: Vec::new(),
                constants: Vec::new(),
                entry_point: Some("fs_canvas".into()),
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                front_face: FrontFace::Ccw,
                cull_mode: None,
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(if key.overlay {
                    CompareFunction::Always
                } else {
                    CompareFunction::GreaterEqual
                }),
                stencil: default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            zero_initialize_workgroup_memory: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct EaselKey {
    format: TextureFormat,
    overlay: bool,
}

impl SpecializedRenderPipeline for EaselPipeline {
    type Key = EaselKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("draw_mode_easel".into()),
            layout: vec![self.layout.clone()],
            immediate_size: 0,
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: Vec::new(),
                constants: Vec::new(),
                entry_point: Some("vs_easel".into()),
                buffers: vec![VertexBufferLayout {
                    array_stride: 28,
                    step_mode: VertexStepMode::Vertex,
                    attributes: vec![
                        VertexAttribute {
                            format: VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 12,
                            shader_location: 1,
                        },
                    ],
                }],
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs: Vec::new(),
                constants: Vec::new(),
                entry_point: Some("fs_easel".into()),
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                front_face: FrontFace::Ccw,
                cull_mode: None,
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(if key.overlay {
                    CompareFunction::Always
                } else {
                    CompareFunction::GreaterEqual
                }),
                stencil: default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            zero_initialize_workgroup_memory: false,
        }
    }
}

#[derive(Resource, Default)]
struct ExtractedDrawCanvas(frame::DrawCanvas);

struct CanvasTexture {
    texture: Texture,
    bind_group: BindGroup,
    width: u32,
    height: u32,
    revision: u64,
}

#[derive(Resource, Default)]
struct DrawCanvasGpu {
    canvas: Option<CanvasTexture>,
}

fn init_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    device: Res<RenderDevice>,
) {
    let params = device.create_buffer(&BufferDescriptor {
        label: Some("draw_mode_canvas_params"),
        size: PARAMS_SIZE,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = BindGroupLayoutDescriptor::new(
        "draw_mode_canvas_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer_sized(false, NonZeroU64::new(PARAMS_SIZE)),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let easel_buffer = device.create_buffer(&BufferDescriptor {
        label: Some("draw_mode_easel_vertices"),
        size: (EASEL_VERTEX_CAPACITY * core::mem::size_of::<EaselVertex>()) as u64,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    commands.insert_resource(EaselGpu {
        buffer: easel_buffer,
        vertices: Vec::with_capacity(EASEL_VERTEX_CAPACITY),
    });
    commands.insert_resource(EaselPipeline {
        shader: asset_server.load(EASEL_SHADER_PATH),
        layout: layout.clone(),
    });
    commands.insert_resource(DrawCanvasPipeline {
        shader: asset_server.load(SHADER_PATH),
        layout,
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("draw_mode_canvas_sampler"),
            address_mode_u: bevy::render::render_resource::AddressMode::ClampToEdge,
            address_mode_v: bevy::render::render_resource::AddressMode::ClampToEdge,
            mag_filter: bevy::render::render_resource::FilterMode::Linear,
            min_filter: bevy::render::render_resource::FilterMode::Linear,
            ..default()
        }),
        params,
    });
}

fn extract_draw_canvas(
    mut extracted: ResMut<ExtractedDrawCanvas>,
    source: Extract<Option<Res<frame::DrawCanvas>>>,
) {
    extracted.0 = match source.as_deref() {
        Some(canvas) => canvas.clone(),
        None => frame::DrawCanvas::default(),
    };
}

fn prepare_draw_canvas(
    extracted: Res<ExtractedDrawCanvas>,
    pipeline: Res<DrawCanvasPipeline>,
    mut gpu: ResMut<DrawCanvasGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
) {
    let Some(pixels) = extracted.0.pixels.as_ref().filter(|_| extracted.0.visible) else {
        gpu.canvas = None;
        return;
    };

    let needs_texture = gpu
        .canvas
        .as_ref()
        .is_none_or(|canvas| canvas.width != pixels.width || canvas.height != pixels.height);
    if needs_texture {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("draw_mode_canvas"),
            size: Extent3d {
                width: pixels.width,
                height: pixels.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(
            "draw_mode_canvas",
            &cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                pipeline.params.as_entire_buffer_binding(),
                &view,
                &pipeline.sampler,
            )),
        );
        gpu.canvas = Some(CanvasTexture {
            texture,
            bind_group,
            width: pixels.width,
            height: pixels.height,
            revision: 0,
        });
    }

    let canvas = gpu.canvas.as_mut().expect("created above");
    if canvas.revision == pixels.revision() {
        return;
    }
    let pixels_guard = pixels.read();
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &canvas.texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &pixels_guard[..],
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(pixels.width * 4),
            rows_per_image: Some(pixels.height),
        },
        Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        },
    );
    canvas.revision = pixels.revision();
}

#[allow(clippy::too_many_arguments)]
fn draw_draw_canvas(
    extracted: Res<ExtractedDrawCanvas>,
    pipeline: Res<DrawCanvasPipeline>,
    easel_pipeline: Res<EaselPipeline>,
    gpu: Res<DrawCanvasGpu>,
    mut specialized: ResMut<SpecializedRenderPipelines<DrawCanvasPipeline>>,
    mut easel_specialized: ResMut<SpecializedRenderPipelines<EaselPipeline>>,
    mut easel: ResMut<EaselGpu>,
    cache: Res<PipelineCache>,
    queue: Res<RenderQueue>,
    views: ViewQuery<(&ViewTarget, &SceneDepthTexture, &ExtractedView)>,
    mut context: RenderContext,
) {
    if !extracted.0.visible {
        return;
    }
    let Some(canvas) = gpu.canvas.as_ref() else {
        return;
    };
    let (target, depth, view) = views.into_inner();
    let clip_from_world = view
        .clip_from_world
        .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
    let uniform = CanvasUniform {
        clip_from_world: clip_from_world.to_cols_array_2d(),
        origin: [
            extracted.0.origin[0],
            extracted.0.origin[1],
            extracted.0.origin[2],
            1.0,
        ],
        right: [
            extracted.0.right[0],
            extracted.0.right[1],
            extracted.0.right[2],
            0.0,
        ],
        up: [extracted.0.up[0], extracted.0.up[1], extracted.0.up[2], 0.0],
    };
    queue.write_buffer(&pipeline.params, 0, bytemuck::bytes_of(&uniform));

    let id = specialized.specialize(
        &cache,
        &pipeline,
        DrawCanvasKey {
            format: target.main_texture_format(),
            overlay: extracted.0.overlay,
        },
    );
    let Some(render_pipeline) = cache.get_render_pipeline(id) else {
        return;
    };

    easel.vertices.clear();
    build_easel(&extracted.0, !extracted.0.pocket, &mut easel.vertices);
    let easel_pipeline_id = (!easel.vertices.is_empty()).then(|| {
        easel_specialized.specialize(
            &cache,
            &easel_pipeline,
            EaselKey {
                format: target.main_texture_format(),
                overlay: extracted.0.overlay,
            },
        )
    });
    if !easel.vertices.is_empty() {
        queue.write_buffer(&easel.buffer, 0, bytemuck::cast_slice(&easel.vertices));
    }

    let attachments = [Some(target.get_unsampled_color_attachment())];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("draw_mode_canvas_pass"),
        color_attachments: &attachments,
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_bind_group(0, &canvas.bind_group, &[]);
    if let Some(id) = easel_pipeline_id
        && let Some(pipeline) = cache.get_render_pipeline(id)
    {
        pass.set_render_pipeline(pipeline);
        pass.set_vertex_buffer(0, easel.buffer.slice(..));
        pass.draw(0..easel.vertices.len() as u32, 0..1);
    }
    pass.set_render_pipeline(render_pipeline);
    pass.draw(0..6, 0..1);
}

fn build_easel(canvas: &frame::DrawCanvas, stand: bool, vertices: &mut Vec<EaselVertex>) {
    let origin = Vec3::from_array(canvas.origin);
    let right = Vec3::from_array(canvas.right);
    let up = Vec3::from_array(canvas.up);
    let right_len = right.length();
    let up_len = up.length();
    if right_len <= 0.0 || up_len <= 0.0 {
        return;
    }
    let right_unit = right / right_len;
    let up_unit = up / up_len;
    let normal = right.cross(up).normalize();
    let thin = (right_len.min(up_len) * 0.06).max(1.5);
    let frame = [0.45, 0.29, 0.15, 1.0];
    let wood = [0.30, 0.18, 0.09, 1.0];
    let tray = [0.36, 0.22, 0.10, 1.0];
    let depth_offset = normal * (thin * 0.6);

    center_quad(
        vertices,
        origin + up + depth_offset + up_unit * (thin * 0.5),
        right_unit * (right_len + thin),
        up_unit * (thin * 0.5),
        frame,
    );
    center_quad(
        vertices,
        origin - up + depth_offset - up_unit * (thin * 0.5),
        right_unit * (right_len + thin),
        up_unit * (thin * 0.5),
        frame,
    );
    center_quad(
        vertices,
        origin - right + depth_offset - right_unit * (thin * 0.5),
        right_unit * (thin * 0.5),
        up_unit * (up_len + thin),
        frame,
    );
    center_quad(
        vertices,
        origin + right + depth_offset + right_unit * (thin * 0.5),
        right_unit * (thin * 0.5),
        up_unit * (up_len + thin),
        frame,
    );
    if !stand {
        return;
    }
    center_quad(
        vertices,
        origin - up + normal * (thin * 2.0) - up_unit * (thin * 1.1),
        right_unit * (right_len + thin),
        normal * (thin * 1.8),
        tray,
    );

    let leg_len = up_len * 1.15;
    let spread = right_len * 0.16;
    let leg_width = right_unit * (thin * 0.8);
    let left_top = origin - right - up - up_unit * thin;
    let right_top = origin + right - up - up_unit * thin;
    let rear_top = origin - up - up_unit * thin;
    let left_foot = left_top - Vec3::Z * leg_len - right_unit * spread - normal * (thin * 0.5);
    let right_foot = right_top - Vec3::Z * leg_len + right_unit * spread - normal * (thin * 0.5);
    let rear_foot = rear_top - Vec3::Z * leg_len - normal * (up_len * 0.65);
    beam(vertices, left_top, left_foot, leg_width, wood);
    beam(vertices, right_top, right_foot, leg_width, wood);
    beam(vertices, rear_top, rear_foot, leg_width, wood);
    center_quad(
        vertices,
        origin - up - up_unit * (leg_len * 0.55),
        right_unit * (right_len * 0.55),
        up_unit * (thin * 0.6),
        wood,
    );
}

fn center_quad(
    vertices: &mut Vec<EaselVertex>,
    center: Vec3,
    right: Vec3,
    up: Vec3,
    color: [f32; 4],
) {
    let a = center - right - up;
    let b = center + right - up;
    let c = center + right + up;
    let d = center - right + up;
    push_triangle(vertices, a, b, c, color);
    push_triangle(vertices, a, c, d, color);
}

fn beam(vertices: &mut Vec<EaselVertex>, from: Vec3, to: Vec3, width: Vec3, color: [f32; 4]) {
    center_quad(vertices, (from + to) * 0.5, width, (to - from) * 0.5, color);
}

fn push_triangle(vertices: &mut Vec<EaselVertex>, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
    for position in [a, b, c] {
        vertices.push(EaselVertex {
            position: position.to_array(),
            color,
        });
    }
}

pub(crate) fn register(app: &mut App) {
    bevy::asset::embedded_asset!(app, "draw_canvas.wgsl");
    bevy::asset::embedded_asset!(app, "draw_easel.wgsl");
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<ExtractedDrawCanvas>()
        .init_resource::<DrawCanvasGpu>()
        .init_resource::<SpecializedRenderPipelines<DrawCanvasPipeline>>()
        .init_resource::<SpecializedRenderPipelines<EaselPipeline>>()
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(ExtractSchedule, extract_draw_canvas)
        .add_systems(
            Render,
            prepare_draw_canvas.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            Core3d,
            draw_draw_canvas
                .in_set(Core3dSystems::PostProcess)
                .after(super::postfx::PostFxSet)
                .in_set(DrawCanvasSet),
        );
}
