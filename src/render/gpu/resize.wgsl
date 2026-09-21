// Separable Lanczos3 with CPU-generated coefficients matching image::imageops.
struct Resize { size:vec4<u32>, mode:vec4<u32> }
@group(0) @binding(0) var<uniform> u:Resize;
@group(0) @binding(1) var<storage,read> source:array<u32>;
@group(0) @binding(2) var<storage,read> kernels:array<vec4<u32>>;
@group(0) @binding(3) var<storage,read> weights:array<f32>;
@group(0) @binding(4) var<storage,read_write> intermediate:array<vec4<f32>>;
@group(0) @binding(5) var<storage,read_write> resized:array<u32>;
fn premultiplied(value:u32) -> vec4<f32> {
    if u.mode.y!=0u {return vec4(f32(value),0.0,0.0,0.0);}
    let rgba=vec4(value&255u,(value>>8u)&255u,(value>>16u)&255u,value>>24u);
    return vec4(vec3<f32>((rgba.rgb*rgba.a+vec3(127u))/vec3(255u)),f32(rgba.a));
}
@compute @workgroup_size(16,16)
fn resize(@builtin(global_invocation_id) id:vec3<u32>) {
    if u.mode.x==0u {
        if id.x>=u.size.x || id.y>=u.size.w {return;}
        let kernel=kernels[id.y]; var color=vec4(0.0);
        for(var i=0u;i<kernel.y;i++){color+=premultiplied(source[(kernel.x+i)*u.size.x+id.x])*weights[kernel.z+i];}
        intermediate[id.y*u.size.x+id.x]=color;
    } else {
        if id.x>=u.size.z || id.y>=u.size.w {return;}
        let kernel=kernels[u.size.w+id.x]; var color=vec4(0.0);
        for(var i=0u;i<kernel.y;i++){color+=intermediate[id.y*u.size.x+kernel.x+i]*weights[kernel.z+i];}
        var rgba=vec4<u32>(floor(clamp(color,vec4(0.0),vec4(255.0))+vec4(0.5)));
        if u.mode.y!=0u {resized[id.y*u.size.z+id.x]=rgba.x; return;}
        if rgba.a>0u {rgba=vec4(min((rgba.rgb*255u+vec3(rgba.a/2u))/rgba.a,vec3(255u)),rgba.a);}
        resized[id.y*u.size.z+id.x]=rgba.x|(rgba.y<<8u)|(rgba.z<<16u)|(rgba.a<<24u);
    }
}
