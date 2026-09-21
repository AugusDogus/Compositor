// MetalBrushCoverage.swift's continuous deposition integral, on Vulkan via wgpu.
// The caller restores the provisional tail before dispatch, so each input contains
// only settled density. Unselected paint/erase also blends original stroke pixels;
// other tools apply color, selection, and stroke opacity on the CPU afterward.
struct Parameters {
    mapping: vec4<f32>,
    geometry: vec4<f32>,
    clip: vec4<f32>,
    segment: vec4<f32>,
    size: vec4<u32>,
    spacing: vec4<f32>,
    color: vec4<f32>,
}
struct Output { density: f32, changed: u32, color: u32 }
@group(0) @binding(0) var<uniform> u: Parameters;
@group(0) @binding(1) var<storage, read> previous: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<Output>;
@group(0) @binding(3) var<storage, read> original: array<u32>;

fn tip_density(distance_squared: f32) -> f32 {
    let t = clamp((sqrt(distance_squared) / u.geometry.z - u.geometry.w) / (1.0 - u.geometry.w), 0.0, 1.0);
    let coverage = max(0.0, (exp(-2.5 * t * t) - exp(-2.5)) / (1.0 - exp(-2.5)));
    return -log(max(1.0 - coverage, 0.001));
}
fn deposit(p: vec2<f32>) -> f32 {
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
            result[index].color = pack4x8unorm(color);
        }
    }
}
