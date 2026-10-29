struct CanvasUniform {
    clip_from_world: mat4x4<f32>,
    origin: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
};

@group(0) @binding(0) var<uniform> canvas: CanvasUniform;

struct EaselVertex {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct EaselOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_easel(vertex: EaselVertex) -> EaselOut {
    var out: EaselOut;
    out.clip_position = canvas.clip_from_world * vec4<f32>(vertex.position, 1.0);
    out.color = vertex.color;
    return out;
}

@fragment
fn fs_easel(in: EaselOut) -> @location(0) vec4<f32> {
    return in.color;
}
