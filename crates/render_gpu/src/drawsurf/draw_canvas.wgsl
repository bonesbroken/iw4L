struct CanvasUniform {
    clip_from_world: mat4x4<f32>,
    origin: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
};

@group(0) @binding(0) var<uniform> canvas: CanvasUniform;
@group(0) @binding(1) var canvas_texture: texture_2d<f32>;
@group(0) @binding(2) var canvas_sampler: sampler;

struct CanvasVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_canvas(@builtin(vertex_index) index: u32) -> CanvasVertex {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[index];
    let world = canvas.origin.xyz + canvas.right.xyz * corner.x + canvas.up.xyz * corner.y;
    var out: CanvasVertex;
    out.clip_position = canvas.clip_from_world * vec4<f32>(world, 1.0);
    out.uv = vec2<f32>((corner.x + 1.0) * 0.5, (1.0 - corner.y) * 0.5);
    return out;
}

@fragment
fn fs_canvas(in: CanvasVertex) -> @location(0) vec4<f32> {
    return textureSample(canvas_texture, canvas_sampler, in.uv);
}
