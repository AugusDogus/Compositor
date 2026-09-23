fn vivid_light(b: f32, s: f32) -> f32 {
    if s<=0.5 { if b==1.0 {return 1.0;} if s==0.0 {return 0.0;} return 1.0-min((1.0-b)/(2.0*s),1.0); }
    if b==0.0 {return 0.0;} if s==1.0 {return 1.0;} return min(b/(2.0*(1.0-s)),1.0);
}
fn lum(c: vec3<f32>) -> f32 { return dot(c,vec3(0.3,0.59,0.11)); }
fn sat(c: vec3<f32>) -> f32 { return max(max(c.x,c.y),c.z)-min(min(c.x,c.y),c.z); }
fn set_lum(input: vec3<f32>, level: f32) -> vec3<f32> {
    var c=input+vec3(level-lum(input));
    let low=min(min(c.x,c.y),c.z); let high=max(max(c.x,c.y),c.z);
    if low<0.0 { c=vec3(level)+(c-vec3(level))*level/(level-low); }
    if high>1.0 { c=vec3(level)+(c-vec3(level))*(1.0-level)/(high-level); }
    return c;
}
fn set_sat(c: vec3<f32>, value: f32) -> vec3<f32> {
    let low=min(min(c.x,c.y),c.z); let high=max(max(c.x,c.y),c.z);
    if high<=low { return vec3(0.0); }
    return (c-vec3(low))*value/(high-low);
}
fn blend(mode:u32, bottom:vec4<f32>, top:vec4<f32>) -> vec4<f32> {
    let alpha=top.a+bottom.a*(1.0-top.a);
    if alpha<=0.0 { return vec4(0.0); }
    let b=bottom.rgb; let s=top.rgb;
    var color=s;
    switch mode {
        case 1u: { color=b*s; }
        case 2u: { color=b+s-b*s; }
        case 3u: { color=select(vec3(1.0)-2.0*(vec3(1.0)-b)*(vec3(1.0)-s),2.0*b*s,b<=vec3(0.5)); }
        case 4u: {
            let d=select(sqrt(b),((16.0*b-vec3(12.0))*b+vec3(4.0))*b,b<=vec3(0.25));
            color=select(b+(2.0*s-vec3(1.0))*(d-b),b-(vec3(1.0)-2.0*s)*b*(vec3(1.0)-b),s<=vec3(0.5));
        }
        case 5u: { color=min(b,s); }
        case 6u: { color=max(b,s); }
        case 7u: { color=abs(b-s); }
        case 8u: { for(var i=0u;i<3u;i++){ if b[i]==0.0 {color[i]=0.0;} else if s[i]==1.0 {color[i]=1.0;} else {color[i]=min(b[i]/(1.0-s[i]),1.0);} } }
        case 9u: { for(var i=0u;i<3u;i++){ if b[i]==1.0 {color[i]=1.0;} else if s[i]==0.0 {color[i]=0.0;} else {color[i]=1.0-min((1.0-b[i])/s[i],1.0);} } }
        case 10u: { color=set_lum(set_sat(s,sat(b)),lum(b)); }
        case 11u: { color=set_lum(set_sat(b,sat(s)),lum(b)); }
        case 12u: { color=set_lum(s,lum(b)); }
        case 13u: { color=set_lum(b,lum(s)); }
        case 14u: { color=max(b+s-vec3(1.0),vec3(0.0)); }
        case 15u: { color=min(b+s,vec3(1.0)); }
        case 16u: { color=select(vec3(1.0)-2.0*(vec3(1.0)-b)*(vec3(1.0)-s),2.0*b*s,s<=vec3(0.5)); }
        case 17u: { for(var i=0u;i<3u;i++){color[i]=vivid_light(b[i],s[i]);} }
        case 18u: { color=clamp(b+2.0*s-vec3(1.0),vec3(0.0),vec3(1.0)); }
        case 19u: { color=select(max(b,2.0*s-vec3(1.0)),min(b,2.0*s),s<=vec3(0.5)); }
        case 20u: { for(var i=0u;i<3u;i++){color[i]=select(0.0,1.0,vivid_light(b[i],s[i])>=0.5);} }
        case 21u: { color=b+s-2.0*b*s; }
        case 22u: { color=max(b-s,vec3(0.0)); }
        case 23u: { for(var i=0u;i<3u;i++){if s[i]==0.0 {color[i]=1.0;} else {color[i]=min(b[i]/s[i],1.0);}} }
        default: {}
    }
    return vec4((top.a*((1.0-bottom.a)*s+bottom.a*color)+bottom.a*(1.0-top.a)*b)/alpha,alpha);
}
