fn decode_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
    var hue = input.params[0].x;
    var saturation = input.uv.x;
    var brightness = 1.0 - input.uv.y;
    var alpha = 1.0;
    if input.params[0].y > 1.5 {
        let delta = input.uv - vec2<f32>(0.5);
        let distance = length(delta);
        hue = atan2(-delta.y, delta.x) * 57.2957795;
        saturation = 1.0;
        brightness = 1.0;
        let edge = max(fwidth(distance), 0.0001);
        alpha = (1.0 - smoothstep(0.5 - edge, 0.5, distance)) * 0.85;
    } else if input.params[0].y > 0.5 {
        hue = (1.0 - input.uv.y) * 360.0;
        saturation = 1.0;
        brightness = 1.0;
    }
    let h = fract(hue / 360.0) * 6.0;
    let c = brightness * saturation;
    let x = c * (1.0 - abs(h % 2.0 - 1.0));
    var rgb: vec3<f32>;
    if h < 1.0 { rgb = vec3<f32>(c, x, 0.0); }
    else if h < 2.0 { rgb = vec3<f32>(x, c, 0.0); }
    else if h < 3.0 { rgb = vec3<f32>(0.0, c, x); }
    else if h < 4.0 { rgb = vec3<f32>(0.0, x, c); }
    else if h < 5.0 { rgb = vec3<f32>(x, 0.0, c); }
    else { rgb = vec3<f32>(c, 0.0, x); }
    // HSB and the RGB fields use encoded sRGB. QuickGUI shaders return linear light.
    return vec4<f32>(decode_srgb(rgb + vec3<f32>(brightness - c)), alpha);
}
