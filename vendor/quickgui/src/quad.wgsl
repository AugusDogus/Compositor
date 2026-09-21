struct ViewUniform {
    viewport: vec2<f32>,
    scale: f32,
    _padding: f32,
}

struct GradientRecord {
    // Kind, interpolation space, stop count, unused.
    header: vec4<f32>,
    // Linear start/end XY, radial center and radii, or conic center and start angle.
    geometry: vec4<f32>,
    positions_low: vec4<f32>,
    positions_high: vec4<f32>,
    colors: array<vec4<f32>, 8>,
}

@group(0) @binding(0)
var<uniform> view: ViewUniform;

@group(1) @binding(0)
var<storage, read> gradients: array<GradientRecord>;

struct VertexInput {
    @location(0) geometry: vec4<f32>,
    @location(1) primary: vec4<f32>,
    @location(2) secondary: vec4<f32>,
    @location(3) clip: vec4<f32>,
    // mode, subject radius or spread, border width, blur radius
    @location(4) params: vec4<f32>,
    @location(5) subject: vec4<f32>,
    // Corner radii ordered top-left, top-right, bottom-right, bottom-left.
    @location(6) corners: vec4<f32>,
    // Gradient index (negative when absent), border style, unused, unused.
    @location(7) effects: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) logical_position: vec2<f32>,
    @location(1) geometry: vec4<f32>,
    @location(2) primary: vec4<f32>,
    @location(3) secondary: vec4<f32>,
    @location(4) clip: vec4<f32>,
    @location(5) params: vec4<f32>,
    @location(6) subject: vec4<f32>,
    @location(7) corners: vec4<f32>,
    @location(8) effects: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput, @builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vertex_index];
    let logical_position = input.geometry.xy + corner * input.geometry.zw;
    let physical_position = logical_position * view.scale;

    var output: VertexOutput;
    output.position = vec4<f32>(
        physical_position.x / view.viewport.x * 2.0 - 1.0,
        1.0 - physical_position.y / view.viewport.y * 2.0,
        0.0,
        1.0,
    );
    output.logical_position = logical_position;
    output.geometry = input.geometry;
    output.primary = input.primary;
    output.secondary = input.secondary;
    // Keep clipping in the same logical coordinate space as the primitive. During a macOS live
    // resize the Metal drawable may intentionally remain larger than the current viewport; the
    // fragment builtin position is then expressed in drawable pixels and cannot be compared to
    // current-viewport pixels without clipping the primitive by the resize ratio a second time.
    output.clip = input.clip;
    output.params = input.params;
    output.subject = input.subject;
    output.corners = input.corners;
    output.effects = input.effects;
    return output;
}

// Select the radius of the quadrant containing `point` inside a box of `size`.
fn corner_radius(point: vec2<f32>, size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let right = point.x > size.x * 0.5;
    let bottom = point.y > size.y * 0.5;
    let top_radius = select(radii.x, radii.y, right);
    let bottom_radius = select(radii.w, radii.z, right);
    return select(top_radius, bottom_radius, bottom);
}

fn rounded_box_distance(point: vec2<f32>, size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let half_size = size * 0.5;
    let radius = corner_radius(point, size, radii);
    let safe_radius = clamp(radius, 0.0, min(half_size.x, half_size.y));
    let q = abs(point - half_size) - half_size + vec2<f32>(safe_radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - safe_radius;
}

fn rounded_rect_distance(point: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>) -> f32 {
    return rounded_box_distance(point - rect.xy, rect.zw, radii);
}

// Shape-kind branches use flat per-instance data. Fragment quads evaluate the
// same SDF, but WebGPU cannot prove that across the shared coverage helper.
@diagnostic(off, derivative_uniformity)
fn coverage(distance: f32) -> f32 {
    let antialias_width = max(fwidth(distance), 0.0001);
    return 1.0 - smoothstep(-antialias_width, antialias_width, distance);
}

// Maximum absolute error is about 1.5e-7, which is far below an 8-bit alpha step.
fn erf_approx(value: f32) -> f32 {
    let x = abs(value);
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let polynomial = (((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t
        - 0.284496736) * t + 0.254829592) * t);
    let result = 1.0 - polynomial * exp(-x * x);
    return select(-result, result, value >= 0.0);
}

fn blurred_coverage(distance: f32, blur_radius: f32) -> f32 {
    let crisp = coverage(distance);
    if blur_radius <= 0.001 {
        return crisp;
    }
    // CSS blur radii map closely to a Gaussian whose standard deviation is half the radius.
    let sigma = max(blur_radius * 0.5, 0.0001);
    let blurred = 0.5 * (1.0 - erf_approx(distance / (1.41421356237 * sigma)));
    return clamp(blurred, 0.0, 1.0);
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

fn gradient_stop_position(gradient_index: u32, index: u32) -> f32 {
    var source = gradients[gradient_index].positions_high;
    if index < 4u {
        source = gradients[gradient_index].positions_low;
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

fn gradient_amount(gradient_index: u32, position: vec2<f32>) -> f32 {
    let kind = gradients[gradient_index].header.x;
    let geometry = gradients[gradient_index].geometry;
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

fn gradient_color(gradient_index: u32, position: vec2<f32>) -> vec4<f32> {
    let header = gradients[gradient_index].header;
    let count = u32(max(header.z, 0.0));
    if count == 0u {
        return vec4<f32>(0.0);
    }
    if count == 1u {
        return gradients[gradient_index].colors[0];
    }
    let amount = gradient_amount(gradient_index, position);
    var previous = gradient_stop_position(gradient_index, 0u);
    if amount <= previous {
        return gradients[gradient_index].colors[0];
    }
    for (var index = 1u; index < count; index = index + 1u) {
        let current = gradient_stop_position(gradient_index, index);
        if amount <= current {
            var blend = 1.0;
            if current > previous {
                blend = (amount - previous) / (current - previous);
            }
            return interpolate_color(
                gradients[gradient_index].colors[index - 1u],
                gradients[gradient_index].colors[index],
                blend,
                header.y,
            );
        }
        previous = current;
    }
    return gradients[gradient_index].colors[count - 1u];
}

// The declared solid fill, or the resolved gradient sample when the instance carries one.
fn resolved_fill(input: VertexOutput) -> vec4<f32> {
    if input.effects.x < 0.0 {
        return input.primary;
    }
    return gradient_color(u32(input.effects.x), input.logical_position);
}

const QUARTER_TURN: f32 = 1.5707963268;

// Arc-length position of `point` along the outer rounded rectangle, plus that outline's total
// length. Straight edges and quarter arcs are measured exactly, so dashes stay evenly spaced
// around corners instead of restarting on every side.
fn perimeter_position(point: vec2<f32>, size: vec2<f32>, radii: vec4<f32>) -> vec2<f32> {
    let top_left = radii.x;
    let top_right = radii.y;
    let bottom_right = radii.z;
    let bottom_left = radii.w;
    let top_length = max(size.x - top_left - top_right, 0.0);
    let right_length = max(size.y - top_right - bottom_right, 0.0);
    let bottom_length = max(size.x - bottom_right - bottom_left, 0.0);
    let left_length = max(size.y - bottom_left - top_left, 0.0);
    let top_right_arc = top_right * QUARTER_TURN;
    let bottom_right_arc = bottom_right * QUARTER_TURN;
    let bottom_left_arc = bottom_left * QUARTER_TURN;
    let top_left_arc = top_left * QUARTER_TURN;
    let total = top_length + top_right_arc + right_length + bottom_right_arc
        + bottom_length + bottom_left_arc + left_length + top_left_arc;

    let start_right = top_length + top_right_arc;
    let start_bottom_right_arc = start_right + right_length;
    let start_bottom = start_bottom_right_arc + bottom_right_arc;
    let start_bottom_left_arc = start_bottom + bottom_length;
    let start_left = start_bottom_left_arc + bottom_left_arc;
    let start_top_left_arc = start_left + left_length;

    let center_top_left = vec2<f32>(top_left, top_left);
    let center_top_right = vec2<f32>(size.x - top_right, top_right);
    let center_bottom_right = vec2<f32>(size.x - bottom_right, size.y - bottom_right);
    let center_bottom_left = vec2<f32>(bottom_left, size.y - bottom_left);

    var position = 0.0;
    if point.x <= center_top_left.x && point.y <= center_top_left.y {
        let delta = point - center_top_left;
        let angle = clamp(atan2(-delta.y, -delta.x), 0.0, QUARTER_TURN);
        position = start_top_left_arc + angle * top_left;
    } else if point.x >= center_top_right.x && point.y <= center_top_right.y {
        let delta = point - center_top_right;
        let angle = clamp(atan2(delta.x, -delta.y), 0.0, QUARTER_TURN);
        position = top_length + angle * top_right;
    } else if point.x >= center_bottom_right.x && point.y >= center_bottom_right.y {
        let delta = point - center_bottom_right;
        let angle = clamp(atan2(delta.y, delta.x), 0.0, QUARTER_TURN);
        position = start_bottom_right_arc + angle * bottom_right;
    } else if point.x <= center_bottom_left.x && point.y >= center_bottom_left.y {
        let delta = point - center_bottom_left;
        let angle = clamp(atan2(-delta.x, delta.y), 0.0, QUARTER_TURN);
        position = start_bottom_left_arc + angle * bottom_left;
    } else {
        let to_top = point.y;
        let to_right = size.x - point.x;
        let to_bottom = size.y - point.y;
        let to_left = point.x;
        let nearest = min(min(to_top, to_right), min(to_bottom, to_left));
        if nearest == to_top {
            position = clamp(point.x - top_left, 0.0, top_length);
        } else if nearest == to_right {
            position = start_right + clamp(point.y - top_right, 0.0, right_length);
        } else if nearest == to_bottom {
            position = start_bottom
                + clamp((size.x - bottom_right) - point.x, 0.0, bottom_length);
        } else {
            position = start_left + clamp((size.y - bottom_left) - point.y, 0.0, left_length);
        }
    }
    return vec2<f32>(position, total);
}

// Dash coverage along the border outline. The declared period is scaled so a whole number of
// dashes fits the outline, which keeps both ends of every edge closed like CSS.
@diagnostic(off, derivative_uniformity)
fn dash_coverage(
    point: vec2<f32>,
    rect: vec4<f32>,
    radii: vec4<f32>,
    width: f32,
    style: f32,
) -> f32 {
    if style < 0.5 {
        return 1.0;
    }
    let stroke = max(width, 0.0001);
    var dash = 3.0 * stroke;
    var gap = 2.0 * stroke;
    if style > 1.5 {
        dash = stroke;
        gap = stroke;
    }
    let measured = perimeter_position(point - rect.xy, rect.zw, radii);
    let total = max(measured.y, 0.0001);
    let period = max(dash + gap, 0.0001);
    let repeats = max(floor(total / period + 0.5), 1.0);
    let scaled_period = total / repeats;
    let dash_length = dash * (scaled_period / period);
    let phase = measured.x - floor(measured.x / scaled_period) * scaled_period;
    let distance = abs(phase - dash_length * 0.5) - dash_length * 0.5;
    let antialias = max(fwidth(measured.x), 0.0001);
    return 1.0 - smoothstep(-antialias, antialias, distance);
}

fn quickgui_shade_linear(input: VertexOutput) -> vec4<f32> {
    let logical_position = input.logical_position;
    if logical_position.x < input.clip.x || logical_position.y < input.clip.y ||
       logical_position.x >= input.clip.z || logical_position.y >= input.clip.w {
        discard;
    }

    // Framework elements may use a different inside-border width on each edge. Width pairs are
    // scaled only when they would consume more than the entire box, keeping the inner SDF valid.
    if input.params.x > 3.5 {
        let raw_widths = max(input.subject, vec4<f32>(0.0));
        let horizontal_scale = min(
            1.0,
            input.geometry.z / max(raw_widths.y + raw_widths.w, 0.0001),
        );
        let vertical_scale = min(
            1.0,
            input.geometry.w / max(raw_widths.x + raw_widths.z, 0.0001),
        );
        let widths = vec4<f32>(
            raw_widths.x * vertical_scale,
            raw_widths.y * horizontal_scale,
            raw_widths.z * vertical_scale,
            raw_widths.w * horizontal_scale,
        );
        let maximum_width = max(max(widths.x, widths.y), max(widths.z, widths.w));
        let dashes = dash_coverage(
            logical_position,
            input.geometry,
            input.corners,
            maximum_width,
            input.effects.y,
        );
        let fill_color = resolved_fill(input);
        if input.params.y <= 0.0 {
            // Axis-aligned square edges already receive exact coverage from their triangles. Keep
            // their inside edges exact too: partially transparent SDF coverage is interpreted as
            // vibrant content by NSVisualEffectView and produces a bright seam beside the border.
            let right = input.geometry.x + input.geometry.z;
            let bottom = input.geometry.y + input.geometry.w;
            let is_border =
                (widths.x > 0.0 && logical_position.y < input.geometry.y + widths.x) ||
                (widths.y > 0.0 && logical_position.x >= right - widths.y) ||
                (widths.z > 0.0 && logical_position.y >= bottom - widths.z) ||
                (widths.w > 0.0 && logical_position.x < input.geometry.x + widths.w);
            let ring = select(0.0, dashes, is_border);
            // A dashed or dotted gap reveals the element background, which CSS paints out to
            // the border box. Solid borders keep their historical disjoint fill region.
            let inside = select(1.0, 0.0, is_border && input.effects.y < 0.5);
            let border_alpha = input.secondary.a * ring;
            let fill_alpha = fill_color.a * inside;
            let alpha = border_alpha + fill_alpha * (1.0 - border_alpha);
            let rgb = input.secondary.rgb * border_alpha
                + fill_color.rgb * fill_alpha * (1.0 - border_alpha);
            return vec4<f32>(rgb, alpha);
        }

        let outer_distance = rounded_rect_distance(
            logical_position,
            input.geometry,
            input.corners,
        );
        let outer = coverage(outer_distance);
        let inner_rect = vec4<f32>(
            input.geometry.xy + vec2<f32>(widths.w, widths.x),
            max(
                input.geometry.zw - vec2<f32>(widths.w + widths.y, widths.x + widths.z),
                vec2<f32>(0.0),
            ),
        );
        // Each inner corner shrinks by the widest of the two edges meeting there.
        let inner_corners = max(
            input.corners - vec4<f32>(
                max(widths.w, widths.x),
                max(widths.y, widths.x),
                max(widths.y, widths.z),
                max(widths.w, widths.z),
            ),
            vec4<f32>(0.0),
        );
        let inner = coverage(rounded_rect_distance(
            logical_position,
            inner_rect,
            inner_corners,
        ));
        let border_alpha = input.secondary.a * max(outer - inner, 0.0) * dashes;
        if input.effects.y > 0.5 {
            // A dash gap reveals the element background, which CSS paints out to the border box,
            // so the pattern composites over the fill instead of partitioning it.
            let fill_alpha = fill_color.a * outer;
            let alpha = border_alpha + fill_alpha * (1.0 - border_alpha);
            let rgb = input.secondary.rgb * border_alpha
                + fill_color.rgb * fill_alpha * (1.0 - border_alpha);
            return vec4<f32>(rgb, alpha);
        }
        // A solid border and its fill cover complementary regions, so their coverage adds
        // exactly and the shared antialiased edge stays seamless.
        let fill_alpha = fill_color.a * inner;
        let alpha = fill_alpha + border_alpha;
        let rgb = fill_color.rgb * fill_alpha + input.secondary.rgb * border_alpha;
        return vec4<f32>(rgb, alpha);
    }

    // A complete underline span is one instance. The fragment shader evaluates its wave
    // analytically, so long diagnostics do not create CPU-side path vertices or extra draws.
    if input.params.x > 2.5 {
        let thickness = max(input.params.z, 0.0001);
        let wavelength = max(input.params.w, 0.0001);
        let amplitude = max(input.subject.x, 0.0);
        let wave_y = input.params.y + amplitude * sin(
            6.28318530718 * input.logical_position.x / wavelength,
        );
        let distance = abs(input.logical_position.y - wave_y) - thickness * 0.5;
        let wave_coverage = coverage(distance);
        let alpha = input.primary.a * wave_coverage;
        return vec4<f32>(input.primary.rgb * alpha, alpha);
    }

    // A square, borderless quad already has exact coverage from its two triangles. Running an
    // SDF over that geometry antialiases every internal edge independently, which exposes seams
    // between adjacent terminal-cell backgrounds at fractional physical coordinates.
    if input.params.x < 0.5 && input.params.y <= 0.0 && input.params.z <= 0.0 {
        let color = resolved_fill(input);
        let alpha = color.a;
        return vec4<f32>(color.rgb * alpha, alpha);
    }

    // Regular rounded rectangle with an inside border.
    if input.params.x < 0.5 {
        let subject_coverage = coverage(rounded_rect_distance(
            input.logical_position,
            input.subject,
            input.corners,
        ));
        let border_width = min(
            max(input.params.z, 0.0),
            min(input.subject.z, input.subject.w) * 0.5,
        );
        let inner_rect = vec4<f32>(
            input.subject.xy + vec2<f32>(border_width),
            max(input.subject.zw - vec2<f32>(border_width * 2.0), vec2<f32>(0.0)),
        );
        let inner = coverage(rounded_rect_distance(
            input.logical_position,
            inner_rect,
            max(input.corners - vec4<f32>(border_width), vec4<f32>(0.0)),
        ));
        let dashes = dash_coverage(
            input.logical_position,
            input.subject,
            input.corners,
            border_width,
            input.effects.y,
        );
        let color = resolved_fill(input);
        let border_alpha = input.secondary.a * max(subject_coverage - inner, 0.0) * dashes;
        if input.effects.y > 0.5 {
            let fill_alpha = color.a * subject_coverage;
            let alpha = border_alpha + fill_alpha * (1.0 - border_alpha);
            let rgb = input.secondary.rgb * border_alpha
                + color.rgb * fill_alpha * (1.0 - border_alpha);
            return vec4<f32>(rgb, alpha);
        }
        let fill_alpha = color.a * inner;
        let alpha = fill_alpha + border_alpha;
        let rgb = color.rgb * fill_alpha + input.secondary.rgb * border_alpha;
        return vec4<f32>(rgb, alpha);
    }

    // Drop shadow. Its element is painted by a later instance in the same ordered draw.
    if input.params.x < 1.5 {
        let subject_coverage = blurred_coverage(
            rounded_rect_distance(input.logical_position, input.subject, input.corners),
            input.params.w,
        );
        let alpha = input.primary.a * subject_coverage;
        return vec4<f32>(input.primary.rgb * alpha, alpha);
    }

    // Inset shadow: the element is the mask and the translated subject is its clear hole. The
    // hole's corners follow the element's, reduced by the declared spread.
    let hole_corners = max(input.corners - vec4<f32>(input.params.y), vec4<f32>(0.0));
    let subject_coverage = blurred_coverage(
        rounded_rect_distance(input.logical_position, input.subject, hole_corners),
        input.params.w,
    );
    let geometry_coverage = coverage(rounded_rect_distance(
        input.logical_position,
        input.geometry,
        input.corners,
    ));
    let inset_coverage = geometry_coverage * (1.0 - subject_coverage);
    let alpha = input.primary.a * inset_coverage;
    return vec4<f32>(input.primary.rgb * alpha, alpha);
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
