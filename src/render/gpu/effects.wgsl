struct Params { size: vec4<u32>, geometry: vec4<f32>, stroke: vec4<f32>, shadow: vec4<f32>, overlay: vec4<f32>, inner: vec4<f32>, flags: vec4<u32>, glow: vec4<f32>, more: vec4<u32>, insideGlow: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage,read> pixels: array<u32>;
@group(0) @binding(2) var<storage,read> input: array<f32>;
@group(0) @binding(3) var<storage,read_write> output: array<f32>;
@group(0) @binding(4) var<storage,read> ring: array<f32>;
@group(0) @binding(5) var<storage,read> shadow: array<f32>;
@group(0) @binding(6) var<storage,read> inner: array<f32>;
@group(0) @binding(7) var<storage,read_write> result: array<u32>;
@group(0) @binding(8) var<storage,read> glow: array<f32>;
fn alpha(i:u32)->f32 { return f32(pixels[i]>>24u)/255.; }
fn over(base:vec4<f32>,color:vec3<f32>,a:f32)->vec4<f32> { return vec4<f32>(color*a,a)+base*(1.-a); }
@compute @workgroup_size(16,16)
fn effects(@builtin(global_invocation_id) gid:vec3<u32>) {
 let w=p.size.x; let h=p.size.y; if gid.x>=w || gid.y>=h {return;} let i=gid.y*w+gid.x; let phase=p.size.z;
 if phase==0u { output[i]=alpha(i); return; }
 if phase==1u || phase==2u {
  var best=select(0.,1.,p.flags.y==1u); let reach=i32(p.geometry.x);
  for(var offset=-reach;offset<=reach;offset+=1) {
   let x=i32(gid.x)+select(0,offset,phase==1u); let y=i32(gid.y)+select(0,offset,phase==2u);
   var value=0.; if x>=0 && y>=0 && x<i32(w) && y<i32(h) {value=input[u32(y)*w+u32(x)];}
   best=select(max(best,value),min(best,value),p.flags.y==1u);
  }
  output[i]=best; return;
 }
 if phase==3u { output[i]=max(0.,select(input[i]-alpha(i),alpha(i)-input[i],p.flags.y==1u)); return; }
 if phase==4u {
  let s=vec2<f32>(gid.xy)-p.geometry.xy; var value=0.;
  if all(s>=vec2<f32>(0.)) && all(s<=vec2<f32>(f32(w-1u),f32(h-1u))) {
   let a=vec2<u32>(floor(s)); let b=min(a+vec2<u32>(1u),vec2<u32>(w-1u,h-1u)); let f=fract(s);
   value=mix(mix(input[a.y*w+a.x],input[a.y*w+b.x],f.x),mix(input[b.y*w+a.x],input[b.y*w+b.x],f.x),f.y);
  } output[i]=value;return;
 }
 if phase==5u || phase==6u {
  let sigma=p.geometry.x; if sigma<=0. {output[i]=input[i];return;}
  let radius=i32(ceil(sigma*3.)); var total=0.;var weights=0.;
  for(var offset=-radius;offset<=radius;offset+=1) {
   let weight=exp(-f32(offset*offset)/(2.*sigma*sigma));
   let x=clamp(i32(gid.x)+select(0,offset,phase==5u),0,i32(w)-1); let y=clamp(i32(gid.y)+select(0,offset,phase==6u),0,i32(h)-1);
   total+=input[u32(y)*w+u32(x)]*weight;weights+=weight;
  } output[i]=total/weights;return;
 }
 let value=pixels[i]; var source=vec4<f32>(f32(value&255u),f32((value>>8u)&255u),f32((value>>16u)&255u),f32(value>>24u))/255.;
 var color=vec4<f32>(0.);
 if p.flags.z==1u {color=over(color,p.shadow.xyz,shadow[i]*p.shadow.w);}
 if p.more.x==1u {color=over(color,p.glow.xyz,glow[i]*(1.-source.a)*p.glow.w);}
 if p.flags.x==1u && p.flags.y==0u {color=over(color,p.stroke.xyz,ring[i]*p.stroke.w);}
 source=vec4<f32>(mix(source.xyz,p.overlay.xyz,p.overlay.w),source.a);
 if p.more.y==1u {
  let a=clamp(source.a*(1.-input[i])*p.insideGlow.w,0.,1.);
  let combined=a+source.a*(1.-a);
  if combined>0. {source=vec4<f32>((p.insideGlow.xyz*a+source.xyz*source.a*(1.-a))/combined,combined);}
 }
 if p.flags.w==1u {source=vec4<f32>(mix(source.xyz,p.inner.xyz,(1.-inner[i])*p.inner.w),source.a);}
 if p.flags.x==1u && p.flags.y==1u && source.a>0. {source=vec4<f32>(mix(source.xyz,p.stroke.xyz,ring[i]/alpha(i)*p.stroke.w),source.a);}
 color=over(color,source.xyz,source.a);
 if color.a>0. {color=vec4<f32>(color.xyz/color.a,color.a);}
 let bytes=vec4<u32>(round(clamp(color,vec4<f32>(0.),vec4<f32>(1.))*255.));
 result[i]=bytes.x|(bytes.y<<8u)|(bytes.z<<16u)|(bytes.w<<24u);
}
