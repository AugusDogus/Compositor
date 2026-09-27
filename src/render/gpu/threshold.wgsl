@group(0) @binding(0) var<uniform> parameters: vec4<u32>;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(256)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= parameters.y { return; }
    let packed=source[id.x];
    if (packed >> 24u)==0u { output[id.x]=packed; return; }
    let luma=(packed & 255u)*299u+((packed >> 8u) & 255u)*587u+((packed >> 16u) & 255u)*114u;
    let value=select(0u,255u,luma>=parameters.x*1000u);
    output[id.x]=value | (value << 8u) | (value << 16u) | (packed & 0xff000000u);
}
