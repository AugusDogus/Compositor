struct ViewUniform {
    viewport: vec2<f32>,
    scale: f32,
    padding: f32,
}

@group(0) @binding(0)
var<uniform> view: ViewUniform;

@group(1) @binding(0)
var image_texture: texture_2d<f32>;

@group(1) @binding(1)
var image_sampler: sampler;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) clip: vec4<f32>,
    @location(3) mask: vec4<f32>,
    @location(4) radius_filtered_opacity_padding: vec4<f32>,
    @location(5) color_row_0: vec4<f32>,
    @location(6) color_row_1: vec4<f32>,
    @location(7) color_row_2: vec4<f32>,
    @location(8) color_row_3: vec4<f32>,
    @location(9) color_offsets: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) logical_position: vec2<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) mask: vec4<f32>,
    @location(4) @interpolate(flat) radius_filtered_opacity_padding: vec4<f32>,
    @location(5) @interpolate(flat) color_row_0: vec4<f32>,
    @location(6) @interpolate(flat) color_row_1: vec4<f32>,
    @location(7) @interpolate(flat) color_row_2: vec4<f32>,
    @location(8) @interpolate(flat) color_row_3: vec4<f32>,
    @location(9) @interpolate(flat) color_offsets: vec4<f32>,
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
    let logical_position = input.rect.xy + corner * input.rect.zw;
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
    output.radius_filtered_opacity_padding = input.radius_filtered_opacity_padding;
    output.color_row_0 = input.color_row_0;
    output.color_row_1 = input.color_row_1;
    output.color_row_2 = input.color_row_2;
    output.color_row_3 = input.color_row_3;
    output.color_offsets = input.color_offsets;
    return output;
}

fn linear_to_srgb_component(value: f32) -> f32 {
    let clamped = clamp(value, 0.0, 1.0);
    if clamped <= 0.0031308 {
        return clamped * 12.92;
    }
    return 1.055 * pow(clamped, 1.0 / 2.4) - 0.055;
}

fn srgb_to_linear_component(value: f32) -> f32 {
    let clamped = clamp(value, 0.0, 1.0);
    if clamped <= 0.04045 {
        return clamped / 12.92;
    }
    return pow((clamped + 0.055) / 1.055, 2.4);
}

fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        linear_to_srgb_component(color.r),
        linear_to_srgb_component(color.g),
        linear_to_srgb_component(color.b),
    );
}

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_component(color.r),
        srgb_to_linear_component(color.g),
        srgb_to_linear_component(color.b),
    );
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
    let distance = rounded_rect_distance(
        mask_position,
        input.mask.zw,
        input.radius_filtered_opacity_padding.x,
    );
    let antialias = max(fwidth(distance), 0.001);
    let coverage = clamp(0.5 - distance / antialias, 0.0, 1.0);
    let sampled = textureSample(image_texture, image_sampler, input.uv);
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


    var color = sampled;
    if input.radius_filtered_opacity_padding.y > 0.5 {
        // CSS filter functions are defined on encoded sRGB, so convert around the matrix.
        let encoded = vec4<f32>(linear_to_srgb(sampled.rgb), sampled.a);
        let filtered = vec4<f32>(
            dot(input.color_row_0, encoded) + input.color_offsets.x,
            dot(input.color_row_1, encoded) + input.color_offsets.y,
            dot(input.color_row_2, encoded) + input.color_offsets.z,
            dot(input.color_row_3, encoded) + input.color_offsets.w,
        );
        color = vec4<f32>(
            srgb_to_linear(clamp(filtered.rgb, vec3<f32>(0.0), vec3<f32>(1.0))),
            clamp(filtered.a, 0.0, 1.0),
        );
    }
    let opacity = clamp(input.radius_filtered_opacity_padding.z, 0.0, 1.0);
    return vec4<f32>(color.rgb * color.a, color.a) * coverage * opacity;
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
