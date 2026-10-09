// Tinted textured quad, the only primitive SimpleGraphic draws.
// Port of SimpleGraphic assets/gles/tinted_texture.vert + .frag
// (master @ 8694a07): positions are in screen pixels (top-left origin) and
// mapped with SceneToNdc = mix((-1, 1), (1, -1), pos / u_screenSize); the
// fragment colour is texture * tint. Blending happens in gamma space because
// textures and the target are non-sRGB (UNORM) formats, like the GL renderer.

struct Uniforms {
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) tint: vec4<f32>) -> VsOut {
    var out: VsOut;
    let unit_pos = pos / u.screen_size;
    let ndc = mix(vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), unit_pos);
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = uv;
    out.tint = tint;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * in.tint;
}
