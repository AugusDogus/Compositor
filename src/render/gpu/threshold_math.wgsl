// Same continuous luminance equation as Threshold::apply_rgb.
fn threshold_rgb(rgb:vec3<f32>, level:f32) -> vec3<f32> {
    let channels=rgb*255.0;
    let luma=channels.r*299.0+channels.g*587.0+channels.b*114.0;
    return vec3(select(0.0,1.0,luma>=level*1000.0));
}
