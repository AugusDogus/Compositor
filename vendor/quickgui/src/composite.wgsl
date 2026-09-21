// Compositing-layer shaders.
//
// `composite` draws one bounded offscreen group texture back into its parent target through an
// affine transform, an optional rounded-rectangle mask, an optional color matrix, and one of the
// separable CSS blend modes. `blur` is one axis of a separable Gaussian used for subtree blurs,
// drop shadows, and backdrop blurs.
//
// Every texture in this file holds premultiplied, target-format color, so the composite output is
// premultiplied too.

struct Composite {
    // Inverse of the group's window-space transform, in physical pixels: (a, b, c, d).
    inverse: vec4<f32>,
    // Translation column of that inverse plus the source texture's physical size.
    inverse_translation: vec2<f32>,
    source_size: vec2<f32>,
    // The rectangle this draw covers, in physical pixels: (x, y, width, height).
    quad: vec4<f32>,
    // Axis-aligned clip in physical pixels: (left, top, right, bottom).
    clip: vec4<f32>,
    // Rounded-rectangle mask box in the group's own physical pixels; zero width disables it.
    mask: vec4<f32>,
    // Mask corner radii in physical pixels: (top-left, top-right, bottom-right, bottom-left).
    corners: vec4<f32>,
    // Color matrix rows applied to straight-alpha, encoded color.
    matrix_r: vec4<f32>,
    matrix_g: vec4<f32>,
    matrix_b: vec4<f32>,
    matrix_a: vec4<f32>,
    matrix_offset: vec4<f32>,
    // Drop-shadow tint. A negative alpha means "not a shadow draw".
    tint: vec4<f32>,
    // (opacity, blend code, reads destination, applies color matrix)
    params: vec4<f32>,
    // Physical size of the render target.
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> composite: Composite;
@group(0) @binding(1) var composite_sampler: sampler;
@group(0) @binding(2) var source_texture: texture_2d<f32>;
@group(0) @binding(3) var destination_texture: texture_2d<f32>;

struct CompositeVertex {
    @builtin(position) position: vec4<f32>,
}

const CORNERS = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
);

@vertex
fn composite_vertex(@builtin(vertex_index) index: u32) -> CompositeVertex {
    let corner = CORNERS[index];
    let position = composite.quad.xy + corner * composite.quad.zw;
    let ndc = vec2<f32>(
        position.x / composite.viewport.x * 2.0 - 1.0,
        1.0 - position.y / composite.viewport.y * 2.0,
    );
    var out: CompositeVertex;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    return out;
}

fn rounded_rect_coverage(point: vec2<f32>, box: vec4<f32>, corners: vec4<f32>) -> f32 {
    let half_size = box.zw * 0.5;
    let center = box.xy + half_size;
    let local = point - center;
    var radius: f32;
    if local.y >= 0.0 {
        radius = select(corners.z, corners.w, local.x < 0.0);
    } else {
        radius = select(corners.y, corners.x, local.x < 0.0);
    }
    radius = clamp(radius, 0.0, min(half_size.x, half_size.y));
    let inner = half_size - vec2<f32>(radius, radius);
    let delta = abs(local) - inner;
    let outside = max(delta, vec2<f32>(0.0, 0.0));
    let distance = length(outside) + min(max(delta.x, delta.y), 0.0) - radius;
    return clamp(0.5 - distance, 0.0, 1.0);
}

fn to_straight(color: vec4<f32>) -> vec4<f32> {
    if color.a <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(color.rgb / color.a, color.a);
}

fn encode_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        return value * 12.92;
    }
    return 1.055 * pow(value, 1.0 / 2.4) - 0.055;
}

fn decode_srgb(value: f32) -> f32 {
    if value <= 0.04045 {
        return value / 12.92;
    }
    return pow((value + 0.055) / 1.055, 2.4);
}

fn apply_color_matrix(color: vec4<f32>) -> vec4<f32> {
    let straight = to_straight(color);
    let encoded = straight;
    let filtered = vec4<f32>(
        dot(composite.matrix_r, encoded) + composite.matrix_offset.x,
        dot(composite.matrix_g, encoded) + composite.matrix_offset.y,
        dot(composite.matrix_b, encoded) + composite.matrix_offset.z,
        dot(composite.matrix_a, encoded) + composite.matrix_offset.w,
    );
    let clamped = clamp(filtered, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(clamped.rgb * clamped.a, clamped.a);
}

fn blend_channel(mode: u32, source: f32, backdrop: f32) -> f32 {
    switch mode {
        case 1u: { return source * backdrop; }
        case 2u: { return source + backdrop - source * backdrop; }
        case 3u: { return min(source, backdrop); }
        case 4u: { return max(source, backdrop); }
        case 5u: {
            // Overlay is hard-light with the operands swapped.
            if backdrop <= 0.5 {
                return 2.0 * source * backdrop;
            }
            return 1.0 - 2.0 * (1.0 - source) * (1.0 - backdrop);
        }
        case 6u: { return abs(source - backdrop); }
        case 7u: { return source + backdrop - 2.0 * source * backdrop; }
        case 8u: {
            if source <= 0.5 {
                return 2.0 * source * backdrop;
            }
            return 1.0 - 2.0 * (1.0 - source) * (1.0 - backdrop);
        }
        case 9u: {
            if backdrop <= 0.0 {
                return 0.0;
            }
            if source >= 1.0 {
                return 1.0;
            }
            return min(1.0, backdrop / (1.0 - source));
        }
        case 10u: {
            if backdrop >= 1.0 {
                return 1.0;
            }
            if source <= 0.0 {
                return 0.0;
            }
            return 1.0 - min(1.0, (1.0 - backdrop) / source);
        }
        default: { return source; }
    }
}

@fragment
fn composite_fragment(input: CompositeVertex) -> @location(0) vec4<f32> {
    let point = input.position.xy;
    if point.x < composite.clip.x
        || point.y < composite.clip.y
        || point.x > composite.clip.z
        || point.y > composite.clip.w {
        discard;
    }

    let source_point = vec2<f32>(
        composite.inverse.x * point.x + composite.inverse.z * point.y
            + composite.inverse_translation.x,
        composite.inverse.y * point.x + composite.inverse.w * point.y
            + composite.inverse_translation.y,
    );
    let uv = source_point / composite.source_size;
    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if uv.x >= 0.0 && uv.y >= 0.0 && uv.x <= 1.0 && uv.y <= 1.0 {
        color = textureSample(source_texture, composite_sampler, uv);
    }

    if composite.tint.a >= 0.0 {
        // Drop shadow: keep only the group's coverage and tint it.
        let tint = to_straight(composite.tint);
        color = vec4<f32>(vec3<f32>(encode_srgb(tint.r), encode_srgb(tint.g), encode_srgb(tint.b)) * composite.tint.a, composite.tint.a) * color.a;
    } else if composite.params.w > 0.5 {
        color = apply_color_matrix(color);
    }

    // The mask is evaluated in the group's own coordinate system, so a backdrop clipped to a
    // rounded rectangle travels through the same transform as the element it belongs to.
    if composite.mask.z > 0.0 {
        color = color * rounded_rect_coverage(source_point, composite.mask, composite.corners);
    }

    color = color * composite.params.x;

    let mode = u32(composite.params.y);
    if composite.params.z > 0.5 {
        let backdrop = textureSample(
            destination_texture,
            composite_sampler,
            point / composite.viewport,
        );
        let straight_source = to_straight(color);
        let straight_backdrop = to_straight(backdrop);
        var blended = vec3<f32>(
            blend_channel(mode, straight_source.r, straight_backdrop.r),
            blend_channel(mode, straight_source.g, straight_backdrop.g),
            blend_channel(mode, straight_source.b, straight_backdrop.b),
        );
        // The separable Porter-Duff form of `source over backdrop` with a blend function.
        let out_rgb = color.rgb * (1.0 - backdrop.a)
            + backdrop.rgb * (1.0 - color.a)
            + blended * color.a * backdrop.a;
        let out_alpha = color.a + backdrop.a * (1.0 - color.a);
        return vec4<f32>(out_rgb, out_alpha);
    }

    return color;
}

struct Blur {
    // (direction x, direction y, taps, step)
    params: vec4<f32>,
    // (inverse sigma squared, source width, source height, unused)
    source: vec4<f32>,
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> blur: Blur;
@group(0) @binding(1) var blur_sampler: sampler;
@group(0) @binding(2) var blur_texture: texture_2d<f32>;

@vertex
fn blur_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = CORNERS[index];
    return vec4<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0);
}

@fragment
fn blur_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let taps = i32(blur.params.z);
    let step = blur.params.w;
    let direction = blur.params.xy;
    let size = blur.source.yz;
    let inverse_variance = blur.source.x;
    var total = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    var weight_sum = 0.0;
    for (var index = -taps; index <= taps; index = index + 1) {
        let offset = f32(index) * step;
        let weight = exp(-0.5 * offset * offset * inverse_variance);
        let sample_point = position.xy + direction * offset;
        let uv = sample_point / size;
        var sample = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        if uv.x >= 0.0 && uv.y >= 0.0 && uv.x <= 1.0 && uv.y <= 1.0 {
            sample = textureSample(blur_texture, blur_sampler, uv);
        }
        total = total + sample * weight;
        weight_sum = weight_sum + weight;
    }
    if weight_sum <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return total / weight_sum;
}
