@group(0) @binding(0) var<uniform> parameters: vec4<u32>;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(256)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= parameters.y {return;}
    let packed=source[id.x];
    if (packed >> 24u)==0u || parameters.x==256u {output[id.x]=packed; return;}
    let channels=vec3(packed & 255u,(packed >> 8u) & 255u,(packed >> 16u) & 255u);
    let bins=channels*parameters.x/256u;
    let denominator=parameters.x-1u;
    let rgb=(bins*255u+vec3(denominator/2u))/denominator;
    output[id.x]=rgb.x | (rgb.y << 8u) | (rgb.z << 16u) | (packed & 0xff000000u);
}
