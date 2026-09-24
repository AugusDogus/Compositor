struct Params { size:vec4<u32>, a:vec4<f32>, b:vec4<f32>, c:vec4<f32> }
@group(0) @binding(0) var<uniform> params:Params;
@group(0) @binding(1) var<storage,read> input:array<u32>;
@group(0) @binding(2) var<storage,read_write> output:array<u32>;
fn sample_pixel(p:vec2<i32>)->vec4<f32> {
 let bounded=clamp(p,vec2<i32>(0),vec2<i32>(params.size.xy)-vec2<i32>(1));
 let rgba=unpack4x8unorm(input[u32(bounded.y)*params.size.x+u32(bounded.x)]);
 return vec4<f32>(rgba.rgb*rgba.a,rgba.a);
}
@compute @workgroup_size(16,16) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
 if any(id.xy>=params.size.xy) {return;}
 let p=vec3<f32>(vec2<f32>(id.xy)+0.5,1.);
 let z=dot(params.c.xyz,p);
 if abs(z)<0.00000001 {output[id.y*params.size.x+id.x]=0u;return;}
 let uv=vec2<f32>(dot(params.a.xyz,p),dot(params.b.xyz,p))/z;
 if any(uv<vec2<f32>(0.)) || any(uv>=vec2<f32>(1.)) {output[id.y*params.size.x+id.x]=0u;return;}
 let xy=uv*vec2<f32>(params.size.xy)-0.5;
 let lo=vec2<i32>(floor(xy));let f=fract(xy);
 let top=mix(sample_pixel(lo),sample_pixel(lo+vec2<i32>(1,0)),f.x);
 let bot=mix(sample_pixel(lo+vec2<i32>(0,1)),sample_pixel(lo+vec2<i32>(1,1)),f.x);
 let rgba=mix(top,bot,f.y);
 var result=rgba;
 if rgba.a>0. {result=vec4<f32>(rgba.rgb/rgba.a,rgba.a);}
 output[id.y*params.size.x+id.x]=pack4x8unorm(result);
}
