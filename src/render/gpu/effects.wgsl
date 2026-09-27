struct Params { size: vec4<u32>, geometry: vec4<f32>, stroke: vec4<f32>, shadow: vec4<f32>, overlay: vec4<f32>, inner: vec4<f32>, flags: vec4<u32>, glow: vec4<f32>, more: vec4<u32>, insideGlow: vec4<f32>, pattern: vec4<u32>, tile: vec4<f32>, gradient: vec4<u32>, gradientSettings:vec4<f32>, gradientGeometry:vec4<i32>, gradientBounds:vec4<i32>, gradientColors:array<vec4<f32>,32>, gradientOpacity:array<vec4<f32>,32>, gradientKeys:array<vec4<i32>,16>, bevelLight:vec4<f32>, bevelSettings:vec4<f32> }
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
fn pattern_sample(point:vec2<f32>) -> vec4<f32> {
 let position=point/p.tile.x-vec2(0.5); let base=floor(position); let f=fract(position);
 let size=vec2<f32>(p.pattern.yz); var total=vec4<f32>(0.);
 for(var y=0u;y<2u;y++){for(var x=0u;x<2u;x++){
  let at=base+vec2<f32>(f32(x),f32(y));
  let wrapped=vec2<u32>(at-floor(at/size)*size);
  let packed=pixels[p.pattern.x+wrapped.y*p.pattern.y+wrapped.x];
  let sample=vec4<f32>(f32(packed&255u),f32((packed>>8u)&255u),f32((packed>>16u)&255u),f32(packed>>24u))/255.;
  let weight=select(1.-f.x,f.x,x==1u)*select(1.-f.y,f.y,y==1u);
  total+=vec4(sample.rgb*sample.a,sample.a)*weight;
 }}
 if total.a>0. {return vec4(total.rgb/total.a,total.a);}
 return total;
}
fn gradient_boundary(index:u32,opacity:bool) -> i32 {
 let at=index+select(0u,32u,opacity);
 return p.gradientKeys[at/4u][at%4u];
}
fn gradient_stop(stops:array<vec4<f32>,32>,count:u32,t:f32,key:i32,opacity:bool) -> vec3<f32> {
 var left=stops[0];
 var boundary=gradient_boundary(0u,opacity);
 if key<boundary {return left.yzw;}
 for(var i=1u;i<count;i++) {
  let next=stops[i];
  boundary=gradient_boundary(i,opacity);
  if key<boundary {
   if next.x<=left.x {return left.yzw;}
   return mix(left.yzw,next.yzw,clamp((t-left.x)/(next.x-left.x),0.,1.));
  }
  left=next;
 }
 return left.yzw;
}
fn gradient_sample(point:vec2<f32>) -> vec4<f32> {
 let size=vec2<i32>(p.gradientSettings.zw);
 let at=vec2<i32>(clamp(round(point*2.),vec2(0.),vec2<f32>(size*2)));
 let direction=p.gradientGeometry.xy;
 let extent=p.gradientBounds.x;
 var key=at.x*direction.x+at.y*direction.y-p.gradientBounds.y;
 if p.gradientSettings.y==1. {key=extent-key;}
 var t=f32(key)/f32(extent);
 if p.gradient.y==1u {
  let centered=at-size;
  let radius=min(size.x,size.y);
  key=min(centered.x*centered.x+centered.y*centered.y,radius*radius);
  t=sqrt(f32(key))/f32(min(size.x,size.y));
  if p.gradientSettings.y==1. {key=-key;t=1.-t;}
 }
 t=clamp(t,0.,1.);
 return vec4(gradient_stop(p.gradientColors,p.gradient.z,t,key,false),gradient_stop(p.gradientOpacity,p.gradient.w,t,key,true).x);
}
fn bevel_shading(point:vec2<u32>) -> vec2<f32> {
 let w=p.size.x; let h=p.size.y;
 let left=point.y*w+u32(max(i32(point.x)-1,0));
 let right=point.y*w+min(point.x+1u,w-1u);
 let top=u32(max(i32(point.y)-1,0))*w+point.x;
 let bottom=min(point.y+1u,h-1u)*w+point.x;
 let nx=-(output[right]-output[left])*p.bevelLight.w;
 let ny=-(output[bottom]-output[top])*p.bevelLight.w;
 let length=sqrt(nx*nx+ny*ny+1.);
 let delta=(nx*p.bevelLight.x+ny*p.bevelLight.y+p.bevelLight.z)/length-p.bevelLight.z;
 return min(2.*max(vec2(delta,-delta),vec2(0.)),vec2(1.))*p.bevelSettings.yz;
}
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
 if p.pattern.w==1u {
  let tile=pattern_sample(vec2<f32>(gid.xy)+vec2(0.5-p.tile.z));
  source=vec4(mix(source.rgb,tile.rgb,tile.a*p.tile.y),source.a);
 }
 if p.gradient.x==1u {
  let g=gradient_sample(vec2<f32>(gid.xy)+vec2(0.5-bitcast<f32>(p.gradientGeometry.z)));
  source=vec4(mix(source.rgb,g.rgb,g.a*p.gradientSettings.x),source.a);
 }
 source=vec4<f32>(mix(source.xyz,p.overlay.xyz,p.overlay.w),source.a);
 if p.more.y==1u {
  let a=clamp(source.a*(1.-input[i])*p.insideGlow.w,0.,1.);
  let combined=a+source.a*(1.-a);
  if combined>0. {source=vec4<f32>((p.insideGlow.xyz*a+source.xyz*source.a*(1.-a))/combined,combined);}
 }
 if p.flags.w==1u {source=vec4<f32>(mix(source.xyz,p.inner.xyz,(1.-inner[i])*p.inner.w),source.a);}
 if p.flags.x==1u && p.flags.y==1u && source.a>0. {source=vec4<f32>(mix(source.xyz,p.stroke.xyz,ring[i]/alpha(i)*p.stroke.w),source.a);}
 var relief=vec2(0.);
 if p.bevelSettings.x>0. {relief=bevel_shading(gid.xy);}
 if p.bevelSettings.x==1. {
  source=vec4(mix(source.rgb,vec3(1.),relief.x),source.a);
  source=vec4(source.rgb*(1.-relief.y),source.a);
 }
 color=over(color,source.xyz,source.a);
 if p.bevelSettings.x>=2. {
  let coverage=select(1.,1.-alpha(i),p.bevelSettings.x==2.);
  color=over(color,vec3(1.),relief.x*coverage);
  color=over(color,vec3(0.),relief.y*coverage);
 }
 if color.a>0. {color=vec4<f32>(color.xyz/color.a,color.a);}
 let bytes=vec4<u32>(round(clamp(color,vec4<f32>(0.),vec4<f32>(1.))*255.));
 result[i]=bytes.x|(bytes.y<<8u)|(bytes.z<<16u)|(bytes.w<<24u);
}
