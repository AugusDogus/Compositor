// Same continuous equations as selective_color::SelectiveColor::apply_rgb.
fn selective_color_rgb(rgb:vec3<f32>, rows:array<vec4<f32>,9>, relative:bool) -> vec3<f32> {
    let highest=max(rgb.r,max(rgb.g,rgb.b));
    let lowest=min(rgb.r,min(rgb.g,rgb.b));
    let weights=array<f32,9>(
        max(rgb.r-max(rgb.g,rgb.b),0.0),
        max(min(rgb.r,rgb.g)-rgb.b,0.0),
        max(rgb.g-max(rgb.r,rgb.b),0.0),
        max(min(rgb.g,rgb.b)-rgb.r,0.0),
        max(rgb.b-max(rgb.r,rgb.g),0.0),
        max(min(rgb.r,rgb.b)-rgb.g,0.0),
        max(2.0*lowest-1.0,0.0),
        max(1.0-abs(highest-0.5)-abs(lowest-0.5),0.0),
        max(1.0-2.0*highest,0.0)
    );
    let available=vec3(1.0)-rgb;
    let mode_scale=select(vec3(1.0),available,relative);
    var correction=vec3(0.0);
    for (var i=0u; i<9u; i++) {
        let row=rows[i];
        let change=(-(vec3(1.0)+row.rgb)*row.a-row.rgb)*mode_scale;
        correction+=clamp(change,-rgb,available)*weights[i];
    }
    return clamp(rgb+correction,vec3(0.0),vec3(1.0));
}
