//////////////////////////////// Vertex shader ////////////////////////////////

struct Camera {
    view_pos: vec4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) color: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) color: vec3<f32>,
}

@vertex
fn vs_main(
    model: VertexInput
) -> VertexOutput {
    var out: VertexOutput;

    out.clip_position = camera.proj * vec4<f32>(model.position, 0.0, 1.0);
    out.tex_coords = model.tex_coords;
    out.color = model.color;

    return out;
}

//////////////////////////////// Fragment shader ////////////////////////////////

@group(1) @binding(0)
var t_diffuse: texture_2d<f32>;

@group(1) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0) * textureSample(t_diffuse, s_diffuse, in.tex_coords);
}
