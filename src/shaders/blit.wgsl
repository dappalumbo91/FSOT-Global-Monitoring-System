@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_blit(@builtin(vertex_index) vi: u32) -> VsOut {
    // Clip y is up. Texture v = 0 is the top row of the offscreen target.
    var xy = vec2<f32>(-1.0, -1.0);
    if (vi == 1u) {
        xy = vec2<f32>(3.0, -1.0);
    }
    if (vi == 2u) {
        xy = vec2<f32>(-1.0, 3.0);
    }
    var out: VsOut;
    out.clip = vec4<f32>(xy, 0.0, 1.0);
    out.uv = vec2<f32>(xy.x * 0.5 + 0.5, 0.5 - xy.y * 0.5);
    return out;
}

@fragment
fn fs_blit(in: VsOut) -> @location(0) vec4<f32> {
    let rgb = textureSample(src, src_samp, in.uv).rgb;
    let d = length(in.uv * 2.0 - vec2<f32>(1.0));
    let vig = smoothstep(1.2, 0.3, d);
    return vec4<f32>(rgb * mix(0.88, 1.0, vig), 1.0);
}
