struct Params { width:u32, height:u32, amount:f32, noise:f32 }
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var<storage,read> source:array<u32>;
@group(0) @binding(2) var<storage,read_write> luminance:array<f32>;
@group(0) @binding(3) var<storage,read_write> alpha:array<f32>;
@group(0) @binding(4) var<storage,read_write> output:array<u32>;
fn rgba(value:u32)->vec4<f32> {
    return vec4<f32>(f32(value&255u),f32((value>>8u)&255u),f32((value>>16u)&255u),f32(value>>24u));
}
fn luma(pixel:vec4<f32>)->f32 { return dot(pixel.rgb,vec3<f32>(0.299,0.587,0.114)); }
@compute @workgroup_size(16,16)
fn prepare(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=p.width || id.y>=p.height { return; }
    let i=id.y*p.width+id.x;
    let pixel=rgba(source[i]);
    alpha[i]=pixel.a/255.;
    luminance[i]=luma(pixel)*pixel.a/255.;
}
@compute @workgroup_size(16,16)
fn combine(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=p.width || id.y>=p.height { return; }
    let i=id.y*p.width+id.x;
    let original=source[i];
    let pixel=rgba(original);
    if pixel.a==0. || alpha[i]<=0. { output[i]=original; return; }
    let difference=luma(pixel)-luminance[i]/alpha[i];
    var keep=1.;
    if p.noise>0. { keep=min(abs(difference)/(p.noise*0.12),1.); }
    let add=difference*p.amount/100.*keep;
    let rgb=vec3<u32>(floor(clamp(pixel.rgb+vec3<f32>(add),vec3<f32>(0.),vec3<f32>(255.))+vec3<f32>(0.5)));
    output[i]=rgb.r|(rgb.g<<8u)|(rgb.b<<16u)|(original&0xff000000u);
}
