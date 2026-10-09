// Full-screen blit of the offscreen Rgba8Unorm render target to the (non-sRGB) surface view
// (ui doc §5.1, D12; SG renders into an RGB target and blits it, r_main.cpp:982-1020).
// The target is RGB in SG, so the output alpha is 1.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
  var o: VOut;
  let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  o.pos = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
  o.uv = uv;
  return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> { return vec4<f32>(textureSample(src, smp, i.uv).rgb, 1.0); }
