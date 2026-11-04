struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sun_dir: vec4<f32>,
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_surface(
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

fn hash21(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    let q = p3 + dot(p3, p3.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

@fragment
fn fs_star(in: VsOut) -> @location(0) vec4<f32> {
    let dir = normalize(in.world);
    let lon = atan2(dir.x, dir.z);
    let lat = asin(clamp(dir.y, -1.0, 1.0));
    let scale = vec2<f32>(150.0, 96.0);
    let cell_f = vec2<f32>(lon, lat) * scale;
    let cell = floor(cell_f);
    let n = hash21(cell);
    let frac = fract(cell_f) - 0.5;
    let keep = step(hash21(cell + vec2<f32>(19.0, 7.0)), abs(cos(lat)) * 0.92 + 0.08);
    let on = step(0.994, n) * keep;
    let core = smoothstep(0.42, 0.0, length(frac));
    let warm = hash21(cell + vec2<f32>(4.0, 11.0));
    let tint = mix(vec3<f32>(0.68, 0.78, 1.0), vec3<f32>(1.0, 0.90, 0.74), warm);
    let intensity = on * core * (0.18 + 2.4 * pow(n, 40.0)) * globals.params.z;
    return vec4<f32>(to_srgb(tint * intensity), 1.0);
}

@fragment
fn fs_atmo(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(globals.camera_pos.xyz - in.world);
    let ndotv = clamp(dot(n, v), 0.0, 1.0);
    let fres = pow(1.0 - ndotv, 2.7);
    let sun = normalize(globals.sun_dir.xyz);
    let day = clamp(dot(n, sun) * 0.55 + 0.45, 0.0, 1.0);
    let glow = fres * day * globals.params.y;
    if (glow < 0.02) {
        discard;
    }
    let rgb = vec3<f32>(0.42, 0.66, 1.0) * glow * 0.85;
    return vec4<f32>(to_srgb(rgb), 1.0);
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_line(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> LineOut {
    var out: LineOut;
    out.clip = globals.view_proj * vec4<f32>(pos, 1.0);
    out.world = pos;
    out.color = color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world);
    let v = normalize(globals.camera_pos.xyz - in.world);
    let fade = smoothstep(0.02, 0.2, clamp(dot(n, v), 0.0, 1.0));
    return vec4<f32>(to_srgb(in.color.rgb), in.color.a * fade);
}
