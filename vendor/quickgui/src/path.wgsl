struct ViewUniform {
    viewport: vec2<f32>,
    scale: f32,
    _padding: f32,
};

struct PathPaint {
    transform: vec4<f32>,
    clip: vec4<f32>,
    color: vec4<f32>,
    // Gradient kind, interpolation space, stop count, and gradient flag.
    header: vec4<f32>,
    geometry: vec4<f32>,
    positions_low: vec4<f32>,
    positions_high: vec4<f32>,
    colors: array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> view: ViewUniform;
@group(1) @binding(0) var<storage, read> paints: array<PathPaint>;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) barycentric: vec3<f32>,
    @location(2) edge_mask: vec3<f32>,
    @location(3) paint_index: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) logical_position: vec2<f32>,
    @location(1) barycentric: vec3<f32>,
    @location(2) @interpolate(flat) edge_mask: vec3<f32>,
    @location(3) @interpolate(flat) paint_index: u32,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let transform = paints[input.paint_index].transform;
    let logical = input.position * transform.xy + transform.zw;
    let physical = logical * view.scale;
    let clip_position = vec2<f32>(
        physical.x / view.viewport.x * 2.0 - 1.0,
        1.0 - physical.y / view.viewport.y * 2.0,
    );
    var output: VertexOutput;
    output.position = vec4<f32>(clip_position, 0.0, 1.0);
    output.logical_position = logical;
    output.barycentric = input.barycentric;
    output.edge_mask = input.edge_mask;
    output.paint_index = input.paint_index;
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

fn linear_to_oklab(color: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * color.r + 0.5363325363 * color.g + 0.0514459929 * color.b;
    let m = 0.2119034982 * color.r + 0.6806995451 * color.g + 0.1073969566 * color.b;
    let s = 0.0883024619 * color.r + 0.2817188376 * color.g + 0.6299787005 * color.b;
    let l_root = sign(l) * pow(abs(l), 1.0 / 3.0);
    let m_root = sign(m) * pow(abs(m), 1.0 / 3.0);
    let s_root = sign(s) * pow(abs(s), 1.0 / 3.0);
    return vec3<f32>(
        0.2104542553 * l_root + 0.7936177850 * m_root - 0.0040720468 * s_root,
        1.9779984951 * l_root - 2.4285922050 * m_root + 0.4505937099 * s_root,
        0.0259040371 * l_root + 0.7827717662 * m_root - 0.8086757660 * s_root,
    );
}

fn oklab_to_linear(color: vec3<f32>) -> vec3<f32> {
    let l_root = color.x + 0.3963377774 * color.y + 0.2158037573 * color.z;
    let m_root = color.x - 0.1055613458 * color.y - 0.0638541728 * color.z;
    let s_root = color.x - 0.0894841775 * color.y - 1.2914855480 * color.z;
    let l = l_root * l_root * l_root;
    let m = m_root * m_root * m_root;
    let s = s_root * s_root * s_root;
    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

fn interpolate_color(first: vec4<f32>, second: vec4<f32>, amount: f32, space: f32) -> vec4<f32> {
    var first_coordinates: vec3<f32>;
    var second_coordinates: vec3<f32>;
    if space > 1.5 {
        first_coordinates = linear_to_oklab(first.rgb);
        second_coordinates = linear_to_oklab(second.rgb);
    } else if space > 0.5 {
        first_coordinates = linear_to_srgb(first.rgb);
        second_coordinates = linear_to_srgb(second.rgb);
    } else {
        first_coordinates = first.rgb;
        second_coordinates = second.rgb;
    }
    let alpha = mix(first.a, second.a, amount);
    let premultiplied = mix(first_coordinates * first.a, second_coordinates * second.a, amount);
    var coordinates = vec3<f32>(0.0);
    if alpha > 0.000001 {
        coordinates = premultiplied / alpha;
    }
    var rgb: vec3<f32>;
    if space > 1.5 {
        rgb = oklab_to_linear(coordinates);
    } else if space > 0.5 {
        rgb = srgb_to_linear(coordinates);
    } else {
        rgb = coordinates;
    }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
}

fn path_stop_position(index: u32, paint_index: u32) -> f32 {
    var source = paints[paint_index].positions_high;
    if index < 4u {
        source = paints[paint_index].positions_low;
    }
    let lane = index % 4u;
    if lane == 0u {
        return source.x;
    }
    if lane == 1u {
        return source.y;
    }
    if lane == 2u {
        return source.z;
    }
    return source.w;
}

fn path_gradient_amount(paint_index: u32, position: vec2<f32>) -> f32 {
    let kind = paints[paint_index].header.x;
    let geometry = paints[paint_index].geometry;
    if kind < 0.5 {
        let start = geometry.xy;
        let direction = geometry.zw - start;
        let denominator = dot(direction, direction);
        if denominator <= 0.000001 {
            return 0.0;
        }
        return clamp(dot(position - start, direction) / denominator, 0.0, 1.0);
    }
    if kind < 1.5 {
        let radii = max(geometry.zw, vec2<f32>(0.000001));
        return clamp(length((position - geometry.xy) / radii), 0.0, 1.0);
    }
    let delta = position - geometry.xy;
    let angle = atan2(delta.x, -delta.y);
    return fract((angle - geometry.z) / 6.28318530718 + 1.0);
}

fn path_color(paint_index: u32, position: vec2<f32>) -> vec4<f32> {
    let header = paints[paint_index].header;
    if header.w < 0.5 {
        return paints[paint_index].color;
    }
    let count = u32(max(header.z, 0.0));
    if count == 0u {
        return vec4<f32>(0.0);
    }
    if count == 1u {
        return paints[paint_index].colors[0];
    }
    let amount = path_gradient_amount(paint_index, position);
    var previous = path_stop_position(0u, paint_index);
    if amount <= previous {
        return paints[paint_index].colors[0];
    }
    for (var index = 1u; index < count; index = index + 1u) {
        let current = path_stop_position(index, paint_index);
        if amount <= current {
            var blend = 1.0;
            if current > previous {
                blend = (amount - previous) / (current - previous);
            }
            return interpolate_color(
                paints[paint_index].colors[index - 1u],
                paints[paint_index].colors[index],
                blend,
                header.y,
            );
        }
        previous = current;
    }
    return paints[paint_index].colors[count - 1u];
}

fn quickgui_shade_linear(input: VertexOutput) -> vec4<f32> {
    let clip = paints[input.paint_index].clip;


    let derivative = max(fwidth(input.barycentric), vec3<f32>(0.000001));
    if input.logical_position.x < clip.x || input.logical_position.y < clip.y ||
       input.logical_position.x >= clip.z || input.logical_position.y >= clip.w {
        discard;
    }
    let edge_distance = input.barycentric / derivative;
    let masked_distance = select(vec3<f32>(1000000.0), edge_distance, input.edge_mask > vec3<f32>(0.5));
    let coverage = select(
        smoothstep(0.0, 1.0, min(masked_distance.x, min(masked_distance.y, masked_distance.z))),
        clamp(input.barycentric.x, 0.0, 1.0),
        input.edge_mask.x < 0.0,
    );
    let color = path_color(input.paint_index, input.logical_position);
    let alpha = color.a * coverage;
    return vec4<f32>(color.rgb * alpha, alpha);
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
