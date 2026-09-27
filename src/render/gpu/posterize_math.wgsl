// Keep continuous backdrops continuous until binning. Scale byte values first
// so exact byte boundaries remain exact, matching Posterize::apply_rgb.
fn posterize_rgb(rgb:vec3<f32>, levels:f32) -> vec3<f32> {
    if levels==256.0 {return rgb;}
    return clamp(floor((rgb*255.0)*levels/256.0)/(levels-1.0),vec3(0.0),vec3(1.0));
}
