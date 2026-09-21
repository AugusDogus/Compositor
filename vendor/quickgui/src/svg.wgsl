struct ViewUniform {
    viewport: vec2<f32>,
    scale: f32,
    padding: f32,
}

@group(0) @binding(0)
var<uniform> view: ViewUniform;

@group(1) @binding(0)
var svg_mask: texture_2d<f32>;

@group(1) @binding(1)
var svg_sampler: sampler;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) clip: vec4<f32>,
    @location(3) mask: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) scale_and_translation: vec4<f32>,
    @location(6) radius_and_rotation: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) logical_position: vec2<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) mask: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) radius: f32,
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
    let corner = corners[input.vertex_index];
    let local = (corner - vec2<f32>(0.5))
        * input.rect.zw
        * input.scale_and_translation.xy;
    let angle = input.radius_and_rotation.y;
    let sine = sin(angle);
    let cosine = cos(angle);
    let rotated = vec2<f32>(
        local.x * cosine - local.y * sine,
        local.x * sine + local.y * cosine,
    );
    let logical_position = input.rect.xy
        + input.rect.zw * 0.5
        + rotated
        + input.scale_and_translation.zw;
    let physical_position = logical_position * view.scale;
    let ndc = vec2<f32>(
        physical_position.x / view.viewport.x * 2.0 - 1.0,
        1.0 - physical_position.y / view.viewport.y * 2.0,
    );

    var output: VertexOutput;
    output.position = vec4<f32>(ndc, 0.0, 1.0);
    output.uv = input.uv.xy + corner * input.uv.zw;
    output.logical_position = logical_position;
    output.clip = input.clip;
    output.mask = input.mask;
    output.color = input.color;
    output.radius = input.radius_and_rotation.x;
    return output;
}

fn rounded_rect_distance(position: vec2<f32>, size: vec2<f32>, radius_value: f32) -> f32 {
    let radius = min(max(radius_value, 0.0), min(size.x, size.y) * 0.5);
    let centered = abs(position - size * 0.5) - (size * 0.5 - vec2<f32>(radius));
    return length(max(centered, vec2<f32>(0.0)))
        + min(max(centered.x, centered.y), 0.0)
        - radius;
}

fn quickgui_shade_linear(input: VertexOutput) -> vec4<f32> {


    let mask_position = input.logical_position - input.mask.xy;
    let distance = rounded_rect_distance(mask_position, input.mask.zw, input.radius);
    let antialias = max(fwidth(distance), 0.001);
    let coverage = clamp(0.5 - distance / antialias, 0.0, 1.0);
    let alpha = textureSample(svg_mask, svg_sampler, input.uv).r * input.color.a * coverage;
    if input.logical_position.x < input.clip.x
        || input.logical_position.y < input.clip.y
        || input.logical_position.x >= input.clip.z
        || input.logical_position.y >= input.clip.w
    {
        discard;
    }
    if coverage <= 0.0 {
        discard;
    }


    if alpha <= 0.0 {
        discard;
    }
    return vec4<f32>(input.color.rgb * alpha, alpha);
}

// Public paint colors stay linear; UI targets blend encoded sRGB, as native UI toolkits do.
fn quickgui_encode_component(v: f32) -> f32 {
    if v <= 0.0031308 { return 12.92 * v; }
    return 1.055 * pow(max(v, 0.0), 1.0 / 2.4) - 0.055;
}
fn quickgui_encode_output(color: vec4<f32>) -> vec4<f32> {
    if color.a <= 0.0 { return vec4<f32>(0.0); }
    let straight = color.rgb / color.a;
    return vec4<f32>(vec3<f32>(quickgui_encode_component(straight.r), quickgui_encode_component(straight.g), quickgui_encode_component(straight.b)) * color.a, color.a);
}
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return quickgui_encode_output(quickgui_shade_linear(input));
}
