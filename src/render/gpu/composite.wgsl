struct Layer {
    image_map:vec4<f32>, image_dy:vec4<f32>,
    mask_map:vec4<f32>, mask_dy:vec4<f32>,
    image:vec4<u32>, mask:vec4<u32>, info:vec4<u32>, options:vec4<u32>,
}
struct View { size:vec4<u32>, geometry:vec4<f32> }
@group(0) @binding(0) var<uniform> view:View;
@group(0) @binding(1) var<storage,read> layers:array<Layer>;
@group(0) @binding(2) var<storage,read> operations:array<vec2<u32>>;
@group(0) @binding(3) var<storage,read> settings:array<f32>;
@group(0) @binding(4) var<storage,read> assets:array<u32>;
@group(0) @binding(5) var<storage,read_write> output:array<u32>;
fn unit(mapping:vec4<f32>,dy:vec2<f32>,p:vec2<f32>) -> vec2<f32> {return mapping.xy+mapping.zw*p.x+dy*p.y;}
fn rgba(info:vec4<u32>,x:i32,y:i32) -> vec4<f32> {
    let ix=u32(clamp(x,0,i32(info.y)-1)); let iy=u32(clamp(y,0,i32(info.z)-1));
    return unpack4x8unorm(assets[info.x+iy*info.y+ix]);
}
fn image_pixel(layer:Layer,p:vec2<f32>) -> vec4<f32> {
    if layer.image.y==0u {return vec4(0.0);}
    let uv=unit(layer.image_map,layer.image_dy.xy,p);
    if any(uv<vec2(0.0)) || any(uv>=vec2(1.0)) {return vec4(0.0);}
    let xy=uv*vec2<f32>(layer.image.yz);
    if layer.image.w==0u {return rgba(layer.image,i32(xy.x),i32(xy.y));}
    let at=xy-vec2(0.5); let base=vec2<i32>(floor(at)); let f=fract(at);
    var color=vec4(0.0);
    for(var y=0;y<2;y++){for(var x=0;x<2;x++){
        let sample=rgba(layer.image,base.x+x,base.y+y);
        let weight=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
        color+=vec4(sample.rgb*sample.a,sample.a)*weight;
    }}
    if color.a>0.0 {color=vec4(color.rgb/color.a,color.a);}
    return color;
}
fn gray(info:vec4<u32>,x:u32,y:u32) -> f32 {return f32(assets[info.x+y*info.y+x])/255.0;}
fn mask_alpha(layer:Layer,p:vec2<f32>) -> f32 {
    if layer.mask.y==0u {return 1.0;}
    let uv=unit(layer.mask_map,layer.mask_dy.xy,p);
    if any(uv<vec2(0.0)) || any(uv>=vec2(1.0)) {return select(0.0,layer.image_dy.w,layer.options.x!=0u);}
    let xy=uv*vec2<f32>(layer.mask.yz);
    if layer.mask.w==0u {return gray(layer.mask,u32(xy.x),u32(xy.y));}
    let at=clamp(xy-vec2(0.5),vec2(0.0),vec2<f32>(layer.mask.yz)-vec2(1.0));
    let base=vec2<u32>(floor(at)); let next=min(base+vec2(1u),layer.mask.yz-vec2(1u)); let f=fract(at);
    return mix(mix(gray(layer.mask,base.x,base.y),gray(layer.mask,next.x,base.y),f.x),mix(gray(layer.mask,base.x,next.y),gray(layer.mask,next.x,next.y),f.x),f.y);
}
fn own_pixel(layer:Layer,p:vec2<f32>) -> vec4<f32> {
    let color=image_pixel(layer,p);
    return vec4(color.rgb,color.a*layer.image_dy.z*mask_alpha(layer,p));
}
fn coverage(index:u32,p:vec2<f32>) -> f32 {
    var i=index; var alpha=1.0;
    for(var depth=0u;depth<=256u;depth++){
        let layer=layers[i];
        var own=1.0;
        if layer.info.z==0u {own=image_pixel(layer,p).a;}
        alpha*=own*layer.image_dy.z*mask_alpha(layer,p);
        if layer.info.y==0u {return alpha;}
        i=layer.info.y-1u;
    }
    return 0.0;
}
fn adjusted(layer:Layer,p:vec2<f32>,color:vec4<f32>,opacity:f32) -> vec4<f32> {
    let point=view.geometry.xy+(p+vec2(0.5))*view.geometry.zw;
    var rgb=adjust_rgb(color.rgb,layer.options.y,layer.info.w,point);
    if layer.options.y==11u || layer.options.y==12u {rgb=image_pixel(layer,p).rgb;}
    let blended=blend(layer.info.x,vec4(color.rgb,1.0),vec4(rgb,1.0));
    return vec4(mix(color.rgb,blended.rgb,opacity),color.a);
}
@compute @workgroup_size(16,16)
fn composite(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=view.size.x || id.y>=view.size.y {return;}
    let p=vec2<f32>(id.xy+vec2(0u,view.size.z));
    var color=vec4(0.0); var group=vec4(0.0); var group_alpha=0.0; var group_input_alpha=0.0;
    var masks:array<f32,66>; masks[0]=1.0; var depth=0u;
    var opacities:array<f32,66>; opacities[0]=1.0;
    for(var i=0u;i<view.size.w;i++){
        let op=operations[i]; let layer=layers[op.y];
        switch op.x {
            case 0u: {masks[depth+1u]=masks[depth]*mask_alpha(layer,p); opacities[depth+1u]=opacities[depth]*layer.image_dy.z; depth++;}
            case 1u: {depth--;}
            case 2u: {let top=image_pixel(layer,p); color=blend(layer.info.x,color,vec4(top.rgb,coverage(op.y,p)*masks[depth]*opacities[depth]));}
            case 3u: {group=own_pixel(layer,p); group_input_alpha=group.a; group_alpha=group.a*masks[depth]*opacities[depth]; group.a=1.0;}
            case 4u: {let top=own_pixel(layer,p); group=blend(layer.info.x,group,vec4(top.rgb,top.a*opacities[depth]));}
            case 5u: {group=adjusted(layer,p,group,layer.image_dy.z*mask_alpha(layer,p)*opacities[depth]);}
            case 6u: {color=blend(layer.info.x,color,vec4(group.rgb,group_alpha));}
            case 7u: {color=adjusted(layer,p,color,layer.image_dy.z*mask_alpha(layer,p)*masks[depth]*opacities[depth]);}
            case 8u: {color=vec4(group.rgb,group_input_alpha);}
            default: {}
        }
    }
    output[id.y*view.size.x+id.x]=pack4x8unorm(color);
}
