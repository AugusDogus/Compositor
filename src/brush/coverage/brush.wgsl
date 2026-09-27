// MetalBrushCoverage.swift's continuous deposition integral, on Vulkan via wgpu.
// The caller restores the provisional tail before dispatch, so each input contains
// only settled density. Unselected paint/erase/tonal also blends original stroke pixels;
// other tools apply color, selection, and stroke opacity on the CPU afterward.
struct Parameters {
    mapping: vec4<f32>,
    geometry: vec4<f32>,
    clip: vec4<f32>,
    segment: vec4<f32>,
    size: vec4<u32>,
    spacing: vec4<f32>,
    color: vec4<f32>,
    metric: vec4<f32>,
    tip_state: vec4<u32>,
    tip_spacing: vec4<f32>,
    tip_plan: array<vec4<u32>, 4>,
}
struct Output { density: f32, changed: u32, color: u32 }
@group(0) @binding(0) var<uniform> u: Parameters;
@group(0) @binding(1) var<storage, read> previous: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<Output>;
@group(0) @binding(3) var<storage, read> original: array<u32>;
@group(0) @binding(4) var<storage, read> sampled_tip: array<u32>;

fn tip_density(distance_squared: f32) -> f32 {
    let t = clamp((sqrt(distance_squared) / u.geometry.z - u.geometry.w) / (1.0 - u.geometry.w), 0.0, 1.0);
    let coverage = max(0.0, (exp(-2.5 * t * t) - exp(-2.5)) / (1.0 - exp(-2.5)));
    return -log(max(1.0 - coverage, 0.001));
}
fn cell_hash(input: u32) -> u32 {
    var v = (input ^ (input >> 16u)) * 0x7feb352du;
    v = (v ^ (v >> 15u)) * 0x846ca68bu;
    return v ^ (v >> 16u);
}
fn cell_header(dab: u32) -> vec3<u32> {
    if u.tip_state.x == 1u { return vec3<u32>(sampled_tip[0], sampled_tip[1], sampled_tip[2]); }
    var cell = 0u;
    for (var axis = 0u; axis < 4u; axis++) {
        let plan = u.tip_plan[axis];
        var chosen = plan.w;
        if plan.x == 1u { chosen = (dab + 1u) % plan.y; }
        if plan.x == 2u { chosen = cell_hash(dab ^ u.tip_state.z ^ (axis * 0x9e3779b9u)) % plan.y; }
        cell += chosen * plan.z;
    }
    let offset = min(cell, u.tip_state.x - 1u) * 4u;
    return vec3<u32>(sampled_tip[offset], sampled_tip[offset + 1u], sampled_tip[offset + 2u]);
}
fn tip_pixel(p: vec2<i32>, cell: vec3<u32>) -> f32 {
    if any(p < vec2<i32>(0)) || any(p >= vec2<i32>(cell.xy)) { return 0.0; }
    let index = cell.z + u32(p.y) * cell.x + u32(p.x);
    return f32((sampled_tip[index / 4u] >> ((index % 4u) * 8u)) & 255u) / 255.0;
}
fn sampled_alpha(point: vec2<f32>, cell: vec3<u32>) -> f32 {
    let dimensions = vec2<f32>(cell.xy);
    let scale = f32(max(cell.x, cell.y)) / (u.geometry.z * 2.0);
    let edge_axes = clamp((dimensions / (2.0 * scale) - abs(point)) / u.segment.w + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    let edge = edge_axes.x * edge_axes.y;
    if edge == 0.0 { return 0.0; }
    let p = clamp(point * scale + dimensions * 0.5 - 0.5, vec2<f32>(0.0), dimensions - 1.0);
    let lo = vec2<i32>(floor(p));
    let f = fract(p);
    return mix(mix(tip_pixel(lo, cell), tip_pixel(lo + vec2<i32>(1, 0), cell), f.x),
        mix(tip_pixel(lo + vec2<i32>(0, 1), cell), tip_pixel(lo + vec2<i32>(1, 1), cell), f.x), f.y) * edge;
}
fn sampled_deposit(p: vec2<f32>) -> f32 {
    let first = u.tip_spacing.x;
    let spacing = u.tip_spacing.y;
    if first > u.segment.z { return 0.0; }
    let projection = dot(p, u.segment.xy);
    let diameter = u.geometry.z * 2.0;
    let reach = diameter * 0.7071067811865476 + u.segment.w;
    let last = floor((u.segment.z - first) / spacing);
    let lo = max(0.0, ceil((projection - reach - first) / spacing));
    let hi = min(last, floor((projection + reach - first) / spacing));
    if hi < lo { return 0.0; }
    var coverage = 0.0;
    for (var i = u32(lo); i <= u32(hi); i++) {
        coverage = max(coverage, sampled_alpha(p - u.segment.xy * (first + f32(i) * spacing), cell_header(u.tip_state.y + i)));
    }
    return coverage;
}
fn deposit(point: vec2<f32>) -> f32 {
    let p = vec2<f32>(dot(u.metric.xy, point), dot(u.metric.zw, point));
    if u.tip_state.x != 0u { return sampled_deposit(p); }
    let direction = u.segment.xy;
    let length = u.segment.z;
    let projection = dot(p, direction);
    if u.geometry.w >= 1.0 {
        let delta = p - clamp(projection, 0.0, length) * direction;
        return clamp((u.geometry.z - sqrt(dot(delta, delta))) / u.segment.w + 0.5, 0.0, 1.0);
    }
    if length < 0.000001 { return tip_density(dot(p, p)); }
    let perpendicular = p.x * direction.y - p.y * direction.x;
    let perpendicular_squared = perpendicular * perpendicular;
    let radius_squared = u.geometry.z * u.geometry.z;
    if perpendicular_squared >= radius_squared { return 0.0; }
    let reach = sqrt(radius_squared - perpendicular_squared);
    let lo = max(0.0, projection - reach);
    let hi = min(length, projection + reach);
    if hi <= lo { return 0.0; }
    let midpoint = (lo + hi) * 0.5;
    let half_length = (hi - lo) * 0.5;
    let nodes = array<f32, 4>(0.1834346425, 0.5255324099, 0.7966664774, 0.9602898565);
    let weights = array<f32, 4>(0.3626837834, 0.3137066459, 0.2223810345, 0.1012285363);
    var integral = 0.0;
    for (var i = 0u; i < 4u; i++) {
        let a = midpoint - half_length * nodes[i] - projection;
        let b = midpoint + half_length * nodes[i] - projection;
        integral += weights[i] * (tip_density(perpendicular_squared + a * a) + tip_density(perpendicular_squared + b * b));
    }
    return integral * half_length / u.spacing.x;
}
fn alpha(value: f32) -> f32 {
    if u.geometry.w >= 1.0 { return value; }
    return 1.0 - exp(-value);
}
// Must match brush::tonal's reference path, including sRGB transfer and range weights.
fn tonal_color(base: vec4<f32>, amount: f32) -> vec4<f32> {
    if base.a == 0.0 || amount == 0.0 { return base; }
    let luminance = dot(base.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    if u.size.z >= 5u {
        let saturation = 1.0 + select(-amount, amount, u.size.z == 5u);
        return vec4<f32>(clamp(vec3<f32>(luminance) + (base.rgb - luminance) * saturation, vec3<f32>(0.0), vec3<f32>(1.0)), base.a);
    }
    var weight = 1.0;
    if u.size.w == 1u { weight = 1.0 - smoothstep(0.0, 0.75, luminance); }
    if u.size.w == 2u { weight = 4.0 * luminance * (1.0 - luminance); }
    if u.size.w == 3u { weight = smoothstep(0.25, 1.0, luminance); }
    let exposure = exp2(amount * weight * select(1.0, -1.0, u.size.z == 4u));
    let linear = select(pow((base.rgb + 0.055) / 1.055, vec3<f32>(2.4)), base.rgb / 12.92, base.rgb <= vec3<f32>(0.04045));
    let adjusted = clamp(linear * exposure, vec3<f32>(0.0), vec3<f32>(1.0));
    let rgb = select(1.055 * pow(adjusted, vec3<f32>(1.0 / 2.4)) - 0.055, adjusted * 12.92, adjusted <= vec3<f32>(0.0031308));
    return vec4<f32>(rgb, base.a);
}
@compute @workgroup_size(16, 16)
fn brush(@builtin(global_invocation_id) pixel: vec3<u32>) {
    if pixel.x >= u.size.x || pixel.y >= u.size.y { return; }
    let index = pixel.y * u.size.x + pixel.x;
    let before = previous[index];
    result[index] = Output(before, 0u, 0u);
    let p = u.geometry.xy + f32(pixel.x) * u.mapping.xy + f32(pixel.y) * u.mapping.zw;
    if any(p < u.clip.xy) || any(p >= u.clip.zw) { return; }
    let old_alpha = u32(round(255.0 * alpha(before)));
    if old_alpha >= 255u { return; }
    let added = deposit(p);
    var value = before + added;
    if u.geometry.w >= 1.0 { value = max(before, added); }
    let new_alpha = u32(round(255.0 * alpha(value)));
    result[index].density = value;
    if new_alpha > old_alpha {
        result[index].changed = new_alpha;
        if u.size.z != 0u {
            let base = unpack4x8unorm(original[index]);
            let amount = f32(new_alpha) / 255.0 * u.spacing.y;
            var color = vec4<f32>(base.rgb, base.a * (1.0 - amount));
            if u.size.z == 1u {
                let top_alpha = u.color.a * amount;
                let out_alpha = top_alpha + base.a * (1.0 - top_alpha);
                color = vec4<f32>(0.0);
                if out_alpha > 0.0 {
                    color = vec4<f32>((u.color.rgb * top_alpha + base.rgb * base.a * (1.0 - top_alpha)) / out_alpha, out_alpha);
                }
            }
            if u.size.z >= 3u { color = tonal_color(base, amount); }
            result[index].color = pack4x8unorm(color);
            if u.size.z >= 3u && result[index].color == original[index] {
                result[index].changed = 0u;
            }
        }
    }
}
