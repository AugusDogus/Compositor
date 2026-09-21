fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
    let point = input.uv * input.size;
    let phase = input.params[0].xy;
    let spacing = input.params[0].z;
    let scale = input.params[0].w;
    let delta = point - phase;
    let distance = abs(delta - round(delta / spacing) * spacing);
    // A one-device-pixel line integrated across this device pixel's footprint.
    let coverage = clamp(vec2<f32>(1.0) - distance * scale, vec2<f32>(0.0), vec2<f32>(1.0));
    let combined = coverage.x + coverage.y - coverage.x * coverage.y;
    return vec4<f32>(input.params[1].rgb, input.params[1].a * combined);
}
