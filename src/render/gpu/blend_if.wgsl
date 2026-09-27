// Gray Blend If ranges use PSD byte endpoints and straight encoded-sRGB.
fn blend_if_range(points:vec4<u32>,gray:u32)->f32 {
    let p=points*1000u;
    var rising=select(0.0,1.0,gray>=p.x);
    if points.x!=points.y {rising=clamp((f32(gray)-f32(p.x))/f32(p.y-p.x),0.0,1.0);}
    var falling=select(0.0,1.0,gray<=p.w);
    if points.z!=points.w {falling=clamp((f32(p.w)-f32(gray))/f32(p.w-p.z),0.0,1.0);}
    return rising*falling;
}
fn blend_if_gray(rgb:vec3<f32>)->u32 {
    let bytes=vec3<u32>(floor(clamp(rgb,vec3(0.0),vec3(1.0))*255.0+vec3(0.5)));
    return bytes.r*299u+bytes.g*587u+bytes.b*114u;
}
fn blend_if_weight(layer:Layer,source:vec4<f32>,backdrop:vec4<f32>)->f32 {
    var under=1.0;
    if backdrop.a>0.0 {
        under=1.0-backdrop.a*(1.0-blend_if_range(layer.blend_underlying,blend_if_gray(backdrop.rgb)));
    }
    return blend_if_range(layer.blend_source,blend_if_gray(source.rgb))*under;
}
