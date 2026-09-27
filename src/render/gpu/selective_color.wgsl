struct Parameters {
    rows: array<vec4<f32>,9>,
    extent: vec4<u32>,
}
@group(0) @binding(0) var<uniform> parameters: Parameters;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(256)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= parameters.extent.x { return; }
    let packed=source[id.x];
    if (packed >> 24u)==0u { output[id.x]=packed; return; }
    let input=vec3<f32>(f32(packed & 255u),f32((packed >> 8u) & 255u),f32((packed >> 16u) & 255u))/255.0;
    let rgb=selective_color_rgb(input,parameters.rows,parameters.extent.y!=0u);
    let encoded=vec3<u32>(floor(rgb*255.0+0.5));
    output[id.x]=encoded.x | (encoded.y << 8u) | (encoded.z << 16u) | (packed & 0xff000000u);
}
