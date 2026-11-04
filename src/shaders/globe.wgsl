// Day and night imagery. The offscreen target is Unorm, so lighting stays
// linear and the color is encoded to sRGB at the end.

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sun_dir: vec4<f32>,
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var day_tex: texture_2d<f32>;
@group(1) @binding(1) var night_tex: texture_2d<f32>;
@group(1) @binding(2) var tex_samp: sampler;
@group(1) @binding(3) var elev_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_globe(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(pos, 1.0);
    out.world = pos;
    out.normal = normal;
    out.uv = uv;
    return out;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, x <= vec3<f32>(0.0031308));
}

@fragment
fn fs_globe(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(globals.camera_pos.xyz - in.world);
    let sun = normalize(globals.sun_dir.xyz);
    let sun_on = globals.params.x;
    let mu = dot(n, sun);

    let albedo = textureSample(day_tex, tex_samp, in.uv).rgb;
    let lights = textureSample(night_tex, tex_samp, in.uv).rgb;
    let elev = textureSample(elev_tex, tex_samp, in.uv);
    let texel = 1.0 / max(vec2<f32>(textureDimensions(elev_tex, 0)), vec2<f32>(1.0));
    let h_l = textureSample(elev_tex, tex_samp, in.uv - vec2<f32>(texel.x, 0.0)).a;
    let h_r = textureSample(elev_tex, tex_samp, in.uv + vec2<f32>(texel.x, 0.0)).a;
    let h_u = textureSample(elev_tex, tex_samp, in.uv - vec2<f32>(0.0, texel.y)).a;
    let h_d = textureSample(elev_tex, tex_samp, in.uv + vec2<f32>(0.0, texel.y)).a;
    let shade = clamp(1.0 - (h_r - h_l) * 1.5 - (h_d - h_u) * 0.65, 0.82, 1.14);
    let land = smoothstep(0.012, 0.045, elev.a) * globals.params.w;
    let base = mix(albedo, mix(albedo, elev.rgb, 0.36) * mix(1.0, shade, land), land);

    let day_term = smoothstep(-0.03, 0.16, mu);
    let daylight = mix(1.0, day_term, sun_on);
    let ambient = mix(vec3<f32>(0.0), vec3<f32>(0.012, 0.016, 0.030), sun_on);
    let night_mask = (1.0 - smoothstep(-0.40, 0.10, mu)) * sun_on * globals.sun_dir.w;

    let water = smoothstep(0.04, 0.14, albedo.b - albedo.r)
        * smoothstep(0.0, 0.08, albedo.b - albedo.g);
    let h = normalize(sun + v);
    let spec = pow(clamp(dot(n, h), 0.0, 1.0), 64.0) * daylight * water * 0.55;

    let rise = smoothstep(-0.10, 0.0, mu);
    let fall = 1.0 - smoothstep(0.02, 0.22, mu);
    let sunset = vec3<f32>(1.0, 0.42, 0.16) * rise * fall * sun_on * 0.16;

    let ndotv = clamp(dot(n, v), 0.0, 1.0);
    let fres = pow(1.0 - ndotv, 3.0);
    let limb = vec3<f32>(0.32, 0.55, 1.0) * fres * globals.params.y * (0.16 + 0.84 * daylight);

    var color = base * (ambient + vec3<f32>(daylight)) + base * sunset;
    color += vec3<f32>(spec);
    color += lights * night_mask * 1.35;
    color += limb;
    return vec4<f32>(to_srgb(color), 1.0);
}
