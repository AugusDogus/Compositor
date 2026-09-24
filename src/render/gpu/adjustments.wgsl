fn wrap(value:f32, span:f32) -> f32 { return value-floor(value/span)*span; }
fn rgb_hsl(c:vec3<f32>) -> vec3<f32> {
    let high=max(max(c.x,c.y),c.z); let low=min(min(c.x,c.y),c.z);
    let delta=high-low; let light=(high+low)*0.5;
    if delta==0.0 {return vec3(0.0,0.0,light);}
    var hue=(c.x-c.y)/delta+4.0;
    if high==c.x {hue=wrap((c.y-c.z)/delta,6.0);} else if high==c.y {hue=(c.z-c.x)/delta+2.0;}
    return vec3(hue*60.0,delta/(1.0-abs(2.0*light-1.0)),light);
}
fn hsl_rgb(hsl:vec3<f32>) -> vec3<f32> {
    let c=(1.0-abs(2.0*hsl.z-1.0))*hsl.y;
    let x=c*(1.0-abs(wrap(hsl.x/60.0,2.0)-1.0));
    var color=vec3(c,0.0,x);
    switch u32(floor(hsl.x/60.0)) {
        case 0u: {color=vec3(c,x,0.0);} case 1u: {color=vec3(x,c,0.0);}
        case 2u: {color=vec3(0.0,c,x);} case 3u: {color=vec3(0.0,x,c);}
        case 4u: {color=vec3(x,0.0,c);} default: {}
    }
    return color+vec3(hsl.z-c*0.5);
}
fn level(value:f32, index:u32) -> f32 {
    let input=clamp((value*255.0-settings[index])/(settings[index+2u]-settings[index]),0.0,1.0);
    return (settings[index+3u]+pow(input,1.0/settings[index+1u])*(settings[index+4u]-settings[index+3u]))/255.0;
}
fn curve(value:f32, offset:u32) -> f32 {
    let count=u32(settings[offset]);
    var j=0u;
    for(var i=1u;i+1u<count;i++){ if settings[offset+1u+i*3u]<=value { j=i; } }
    let a=offset+1u+j*3u; let b=a+3u;
    let h=settings[b]-settings[a]; let t=clamp((value-settings[a])/h,0.0,1.0);
    let t2=t*t; let t3=t2*t;
    return clamp((2.0*t3-3.0*t2+1.0)*settings[a+1u]+(t3-2.0*t2+t)*h*settings[a+2u]+(-2.0*t3+3.0*t2)*settings[b+1u]+(t3-t2)*h*settings[b+2u],0.0,255.0);
}
fn mix32(input:u32) -> u32 {
    var x=input^(input>>16u); x*=0x7feb352du; x^=x>>15u; x*=0x846ca68bu; return x^(x>>16u);
}
fn noise_unit(key:u32) -> f32 {return f32(mix32(key)>>8u)/16777216.0;}
fn lattice(x:i32,y:i32,seed:u32) -> f32 {
    let h=mix32(bitcast<u32>(x)*0x9E3779B1u ^ mix32(bitcast<u32>(y)*0x85EBCA77u ^ seed));
    return f32(h&65535u)/65535.0+f32(h>>16u)/65535.0-1.0;
}
fn grain_field(point:vec2<f32>, scale:f32, seed:u32) -> f32 {
    let p=point/scale; let ix=i32(floor(p.x)); let iy=i32(floor(p.y));
    let f=fract(p); let t=f*f*(vec2(3.0)-2.0*f);
    return mix(mix(lattice(ix,iy,seed),lattice(ix+1,iy,seed),t.x),mix(lattice(ix,iy+1,seed),lattice(ix+1,iy+1,seed),t.x),t.y)*1.6;
}
fn adjust_rgb(rgb:vec3<f32>, kind:u32, offset:u32, point:vec2<f32>) -> vec3<f32> {
    var result=rgb;
    switch kind {
        case 1u: {
            var hsl=rgb_hsl(rgb); var lightness=0.0;
            if settings[offset]!=0.0 {
                hsl.x=settings[offset+1u]; hsl.y=clamp(settings[offset+2u]/100.0,0.0,1.0); lightness=settings[offset+3u]/100.0;
            } else {
                let i=offset+4u+u32(floor(hsl.x+0.5))*3u;
                hsl.x+=settings[i]; hsl.y=clamp(hsl.y*(1.0+settings[i+1u]/100.0),0.0,1.0); lightness=settings[i+2u]/100.0;
            }
            let amount=clamp(lightness,-1.0,1.0);
            hsl.z=select(hsl.z*(1.0+amount),hsl.z+(1.0-hsl.z)*amount,amount>=0.0);
            hsl.x=wrap(hsl.x,360.0); result=hsl_rgb(hsl);
        }
        case 2u: { for(var i=0u;i<3u;i++){result[i]=level(level(rgb[i],offset+5u*(i+1u)),offset);} }
        case 3u: {
            var channel=offset+1u+u32(settings[offset])*3u;
            for(var i=0u;i<3u;i++){
                result[i]=curve(curve(rgb[i]*255.0,channel),offset)/255.0;
                channel+=1u+u32(settings[channel])*3u;
            }
        }
        case 4u: {
            for(var i=0u;i<3u;i++){
                var linear=rgb[i]/12.92;
                if rgb[i]>0.04045 {linear=pow((rgb[i]+0.055)/1.055,2.4);}
                let value=pow(max(linear*settings[offset]+settings[offset+1u],0.0),settings[offset+2u]);
                result[i]=value*12.92;
                if value>0.0031308 {result[i]=1.055*pow(value,1.0/2.4)-0.055;}
            }
        }
        case 5u: {
            var t=dot(rgb,vec3(0.2126,0.7152,0.0722)); if settings[offset+6u]!=0.0 {t=1.0-t;}
            for(var i=0u;i<3u;i++){result[i]=settings[offset+i]+(settings[offset+i+3u]-settings[offset+i])*t;}
        }
        case 6u: {
            let seed=bitcast<u32>(settings[offset+3u]);
            let low_frequency=grain_field(point,settings[offset+1u],seed);
            let fine=grain_field(point,max(0.5,settings[offset+1u]*0.35),mix32(seed^0xA511E9B3u));
            let noise=mix(low_frequency,fine,settings[offset+2u]/100.0);
            let l=dot(rgb,vec3(0.2126,0.7152,0.0722));
            result+=vec3(noise*settings[offset]/100.0*0.35*(0.4+2.4*l*(1.0-l)));
        }
        case 7u: { result=vec3(1.0)-rgb; }
        case 8u: {
            let high=max(max(rgb.r,rgb.g),rgb.b); let low=min(min(rgb.r,rgb.g),rgb.b); let mid=rgb.r+rgb.g+rgb.b-high-low;
            var primary=4u; var secondary=select(5u,3u,rgb.g>=rgb.r);
            if high==rgb.r {primary=0u;secondary=select(5u,1u,rgb.g>=rgb.b);} else if high==rgb.g {primary=2u;secondary=select(3u,1u,rgb.r>=rgb.b);}
            let gray=clamp(low+(mid-low)*settings[offset+secondary]+(high-mid)*settings[offset+primary],0.0,1.0);
            result=vec3(gray);
            if settings[offset+6u]!=0.0 {result=hsl_rgb(vec3(wrap(settings[offset+7u],360.0),settings[offset+8u],gray));}
        }
        case 9u: {
            for(var i=0u;i<3u;i++) {
                let v=rgb[i];let shadow=clamp((v-0.333)/-0.25+0.5,0.0,1.0)*0.7;
                let highlight=clamp((v+0.333-1.0)/0.25+0.5,0.0,1.0)*0.7;
                let mid=clamp((v-0.333)/0.25+0.5,0.0,1.0)*clamp((v+0.333-1.0)/-0.25+0.5,0.0,1.0)*0.7;
                result[i]=clamp(v+settings[offset+i]*shadow+settings[offset+3u+i]*mid+settings[offset+6u+i]*highlight,0.0,1.0);
            }
            let after=dot(result,vec3(0.299,0.587,0.114));
            if settings[offset+9u]!=0.0 && after>0.0001 {result*=dot(rgb,vec3(0.299,0.587,0.114))/after;}
        }
        case 10u: {
            let p=vec2<i32>(floor(point));
            let base=mix32(bitcast<u32>(settings[offset+3u]) ^ mix32(bitcast<u32>(p.x)*0x9e3779b9u ^ mix32(bitcast<u32>(p.y)*0x85ebca6bu)));
            for(var c=0u;c<3u;c++) {
                let key=select(base+c*0x9e3779b9u,base,settings[offset+2u]!=0.0);
                var n=(noise_unit(key)*2.0-1.0)*settings[offset];
                if settings[offset+1u]!=0.0 {n=sqrt(-2.0*log(1.0-noise_unit(key)))*cos(6.2831853*noise_unit(key^0x68e31da4u))*settings[offset]*(2.0/3.0);}
                result[c]+=n;
            }
        }
        default: {}
    }
    return clamp(result,vec3(0.0),vec3(1.0));
}
