// Tinted textured quad, the only primitive SimpleGraphic draws (ui doc §5.1).
// Port of SimpleGraphic assets/gles/tinted_texture.vert + .frag (research copy master @ 8694a07):
// positions are in screen pixels (top-left origin) mapped with
// SceneToNdc = mix((-1, 1), (1, -1), pos / screen); the fragment colour is texture * tint.
// Blending happens in gamma space because textures and the target are UNORM (D12).

struct Globals { screen: vec2<f32>, _pad: vec2<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;
struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) tint: vec4<f32> };
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) tint: vec4<f32> };
@vertex fn vs(v: VIn) -> VOut {
  var o: VOut;
  o.pos = vec4<f32>(mix(vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), v.pos / g.screen), 0.0, 1.0);
  o.uv = v.uv; o.tint = v.tint; return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> { return textureSample(tex, smp, i.uv) * i.tint; }
