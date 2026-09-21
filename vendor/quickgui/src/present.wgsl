// Presentation of a transparent frame.
//
// QuickGUI blends in linear light, so an sRGB-encoded surface ends a frame holding
// `encode(a * c)` for a pixel of straight linear colour `c` and coverage `a`. The window
// compositor treats that surface as premultiplied in its own encoding, `a * encode(c)`, and the
// gap between the two forms is a light fringe on every anti-aliased edge over a translucent
// backdrop. This pass rewrites each pixel into the form the compositor expects; the sRGB target
// encodes on store, so the fragment returns the linear value that encodes to `a * encode(c)`.

@group(0) @binding(0) var present_texture: texture_2d<f32>;
@group(0) @binding(1) var present_sampler: sampler;

struct PresentVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn present_vertex(@builtin(vertex_index) index: u32) -> PresentVertex {
    // One triangle covering the viewport; the parts outside it are clipped.
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: PresentVertex;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn encode_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        return value * 12.92;
    }
    return 1.055 * pow(value, 1.0 / 2.4) - 0.055;
}

fn decode_srgb(value: f32) -> f32 {
    if value <= 0.04045 {
        return value / 12.92;
    }
    return pow((value + 0.055) / 1.055, 2.4);
}

@fragment
fn present_fragment(input: PresentVertex) -> @location(0) vec4<f32> {
    let color = textureSample(present_texture, present_sampler, input.uv);
    if color.a <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    if color.a >= 1.0 {
        return color;
    }
    let straight = clamp(color.rgb / color.a, vec3<f32>(0.0), vec3<f32>(1.0));
    let expected = vec3<f32>(
        encode_srgb(straight.r),
        encode_srgb(straight.g),
        encode_srgb(straight.b),
    ) * color.a;
    return vec4<f32>(
        decode_srgb(expected.r),
        decode_srgb(expected.g),
        decode_srgb(expected.b),
        color.a,
    );
}
