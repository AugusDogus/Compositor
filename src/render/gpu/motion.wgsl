struct Params { width:u32, height:u32, steps:u32, unused:u32, distance:f32, cosine:f32, sine:f32, padding:f32 }
@group(0) @binding(0) var<uniform> params:Params;
@group(0) @binding(1) var<storage,read> input:array<u32>;
@group(0) @binding(2) var<storage,read_write> output:array<u32>;
fn sample(p:vec2<f32>)->vec4<f32> {
    if any(p<vec2(0.0)) || any(p>=vec2(f32(params.width),f32(params.height))) {return vec4(0.0);}
    let q=p-vec2(0.5);let base=vec2<i32>(floor(q));let f=fract(q);var sum=vec4(0.0);
    for(var y=0;y<2;y++){for(var x=0;x<2;x++){
        let at=clamp(base+vec2(x,y),vec2(0),vec2(i32(params.width)-1,i32(params.height)-1));
        let c=unpack4x8unorm(input[u32(at.y)*params.width+u32(at.x)]);
        sum+=vec4(c.rgb*c.a,c.a)*select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
    }} return sum;
}
@compute @workgroup_size(16,16)
fn motion(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=params.width || id.y>=params.height {return;}
    var sum=vec4(0.0);
    for(var i=0u;i<params.steps;i++) {
        let t=(f32(i)/f32(params.steps-1u)-0.5)*params.distance;
        sum+=sample(vec2<f32>(id.xy)+vec2(0.5)+t*vec2(params.cosine,params.sine));
    }
    var rgb=vec3(0.0);if sum.a>0.0 {rgb=sum.rgb/sum.a;}
    output[id.y*params.width+id.x]=pack4x8unorm(vec4(rgb,sum.a/f32(params.steps)));
}
