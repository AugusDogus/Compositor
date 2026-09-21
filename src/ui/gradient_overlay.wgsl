// Match the source's encoded-sRGB compositing, then return linear color to QuickGUI.
fn guide_over(base: vec4<f32>, gray: f32, alpha: f32) -> vec4<f32> {
    return vec4<f32>(vec3<f32>(gray) * alpha, alpha) + base * (1.0 - alpha);
}

fn guide_coverage(distance: f32, scale: f32) -> f32 {
    return clamp(0.5 - distance * scale, 0.0, 1.0);
}

fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
    let start = input.params[0].xy;
    let end = input.params[0].zw;
    let scale = input.params[1].y;
    let point = input.uv * input.size;
    let delta = end - start;
    let radius = length(delta);
    if radius < 0.0001 {
        return vec4<f32>(0.0);
    }
    let relative = point - start;
    var result = vec4<f32>(0.0);
    if input.params[1].x > 0.5 {
        let radial_distance = abs(length(relative) - radius);
        let angle = atan2(relative.y, relative.x);
        let arc = (angle + select(0.0, 6.28318530718, angle < 0.0)) * radius;
        let phase = arc - floor(arc / 8.0) * 8.0;
        let dash_distance = abs(phase - 2.0) - 2.0;
        let dark = guide_coverage(max(radial_distance - 1.0, dash_distance), scale);
        let light = guide_coverage(max(radial_distance - 0.5, dash_distance), scale);
        result = guide_over(result, 0.0, 0.5 * dark);
        result = guide_over(result, 1.0, 0.8 * light);
    }
    let direction = delta / radius;
    let along = dot(relative, direction);
    let across = abs(relative.x * direction.y - relative.y * direction.x);
    let ends = max(-along, along - radius);
    result = guide_over(result, 0.0, 0.7 * guide_coverage(max(across - 1.5, ends), scale));
    result = guide_over(result, 1.0, guide_coverage(max(across - 0.5, ends), scale));
    let srgb = result.rgb / max(result.a, 0.00001);
    let linear = select(pow((srgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), srgb / 12.92, srgb <= vec3<f32>(0.04045));
    return vec4<f32>(linear, result.a);
}
