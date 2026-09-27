// Matches Camera Raw's vibrance_and_saturation in AdjustPixels.c.
fn vibrance_chroma(rgb: vec3<f32>, factor: f32) -> vec3<f32> {
    let luma = 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
    return clamp(luma + (rgb - luma) * factor, vec3(0.0), vec3(1.0));
}
fn vibrance_rgb(rgb: vec3<f32>, vibrance: f32, saturation: f32) -> vec3<f32> {
    if vibrance == 0.0 && saturation == 0.0 { return rgb; }
    let highest = max(rgb.r, max(rgb.g, rgb.b));
    let chroma = highest - min(rgb.r, min(rgb.g, rgb.b));
    var sat = 0.0;
    if highest > 1e-8 { sat = chroma / highest; }
    var hue = 0.0;
    if chroma > 1e-8 {
        if rgb.r >= rgb.g && rgb.r >= rgb.b {
            hue = 60.0 * ((rgb.g - rgb.b) / chroma);
            if hue < 0.0 { hue += 360.0; }
        } else if rgb.g >= rgb.r && rgb.g >= rgb.b {
            hue = 60.0 * ((rgb.b - rgb.r) / chroma + 2.0);
        } else {
            hue = 60.0 * ((rgb.r - rgb.g) / chroma + 4.0);
        }
    }
    var skin = 0.0;
    if hue >= 10.0 && hue <= 50.0 {
        skin = select((50.0 - hue) / 20.0, (hue - 10.0) / 20.0, hue <= 30.0);
        skin *= clamp((sat - 0.15) / 0.35, 0.0, 1.0);
    }
    var amount = vibrance / 100.0 * (1.0 - sat);
    if vibrance > 0.0 { amount *= 1.0 - 0.7 * skin; }
    return vibrance_chroma(vibrance_chroma(rgb, 1.0 + amount), 1.0 + saturation / 100.0);
}
