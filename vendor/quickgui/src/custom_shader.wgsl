struct ViewUniform {
    viewport: vec2<f32>,
    scale: f32,
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> view: ViewUniform;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) clip: vec4<f32>,
    @location(2) params_0: vec4<f32>,
    @location(3) params_1: vec4<f32>,
    @location(4) params_2: vec4<f32>,
    @location(5) params_3: vec4<f32>,
    @location(6) opacity_and_padding: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) physical_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) logical_position: vec2<f32>,
    @location(2) @interpolate(flat) logical_clip: vec4<f32>,
    @location(3) @interpolate(flat) size: vec2<f32>,
    @location(4) @interpolate(flat) params_0: vec4<f32>,
    @location(5) @interpolate(flat) params_1: vec4<f32>,
    @location(6) @interpolate(flat) params_2: vec4<f32>,
    @location(7) @interpolate(flat) params_3: vec4<f32>,
    @location(8) @interpolate(flat) opacity: f32,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let uv = corners[input.vertex_index];
    let logical_position = input.rect.xy + uv * input.rect.zw;
    let physical = logical_position * view.scale;

    var output: VertexOutput;
    output.physical_position = vec4<f32>(
        physical.x / view.viewport.x * 2.0 - 1.0,
        1.0 - physical.y / view.viewport.y * 2.0,
        0.0,
        1.0,
    );
    output.uv = uv;
    output.logical_position = logical_position;
    output.logical_clip = input.clip;
    output.size = input.rect.zw;
    output.params_0 = input.params_0;
    output.params_1 = input.params_1;
    output.params_2 = input.params_2;
    output.params_3 = input.params_3;
    output.opacity = input.opacity_and_padding.x;
    return output;
}
