struct Parameters {
    image: vec4<u32>, // width, offset, count, mask target
    options: vec4<u32>, // radial, reversed, stop count, full coverage
    x: vec4<f32>,
    y: vec4<f32>,
    gradient: vec4<f32>, // unit direction, inverse length, opacity
}
struct Stop { color: vec4<f32>, position: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Parameters;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
@group(0) @binding(3) var<storage, read> stops: array<Stop>;
@group(0) @binding(4) var<storage, read> coverage: array<u32>;

fn sample(t: f32) -> vec4<f32> {
    if t < stops[0].position.x { return stops[0].color; }
    for (var i=1u; i<p.options.z; i++) {
        if t < stops[i].position.x {
            let left=stops[i-1u]; let right=stops[i];
            let fraction=(t-left.position.x)/(right.position.x-left.position.x);
            return left.color+(right.color-left.color)*fraction;
        }
    }
    return stops[p.options.z-1u].color;
}
fn pack(c: vec4<f32>) -> u32 {
    let bytes=vec4<u32>(floor(clamp(c,vec4(0.),vec4(1.))*255.+0.5));
    return bytes.x | (bytes.y<<8u) | (bytes.z<<16u) | (bytes.w<<24u);
}
@compute @workgroup_size(256)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x>=p.image.z { return; }
    let original=source[id.x];
    var amount=1.;
    if p.options.w==0u { amount=f32(coverage[id.x])/255.; }
    if amount==0. { output[id.x]=original; return; }
    let index=p.image.y+id.x;
    let pixel=vec2<f32>(f32(index%p.image.x)+0.5,f32(index/p.image.x)+0.5);
    let point=vec2<f32>(dot(p.x.xy,pixel)+p.x.z,dot(p.y.xy,pixel)+p.y.z);
    var t=dot(point,p.gradient.xy)*p.gradient.z;
    if p.options.x!=0u { t=length(point)*p.gradient.z; }
    t=clamp(t,0.,1.);
    if p.options.y!=0u { t=1.-t; }
    var top=sample(t);
    top.a*=p.gradient.w*amount;
    if top.a==0. { output[id.x]=original; return; }
    if p.image.w!=0u {
        let before=f32(original)/255.;
        let gray=dot(top.rgb,vec3<f32>(0.2126,0.7152,0.0722));
        output[id.x]=u32(floor((before+(gray-before)*top.a)*255.+0.5));
    } else {
        let bottom=vec4<f32>(f32(original&255u),f32((original>>8u)&255u),f32((original>>16u)&255u),f32(original>>24u))/255.;
        let alpha=top.a+bottom.a*(1.-top.a);
        let rgb=(top.rgb*top.a+bottom.rgb*bottom.a*(1.-top.a))/alpha;
        output[id.x]=pack(vec4<f32>(rgb,alpha));
    }
}
