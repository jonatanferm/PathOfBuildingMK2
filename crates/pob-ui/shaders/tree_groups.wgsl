// Group-background pipeline: draws one textured quad per passive-tree group
// (Small / Medium / Large halo), sampled from `group-background.png`.
// Renders BEFORE edges + nodes so it sits beneath them.

struct Uniforms {
    viewport_center: vec2<f32>,
    zoom: f32,
    _pad0: f32,
    viewport_size: vec2<f32>,
    pixels_per_point: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Instance {
    @location(0) world_pos: vec2<f32>,
    @location(1) world_size: vec2<f32>,
    @location(2) uv_rect: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

fn quad_corner(idx: u32) -> vec2<f32> {
    var lookup = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5),
        vec2<f32>( 0.5, -0.5),
        vec2<f32>( 0.5,  0.5),
        vec2<f32>(-0.5, -0.5),
        vec2<f32>( 0.5,  0.5),
        vec2<f32>(-0.5,  0.5),
    );
    return lookup[idx];
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, inst: Instance) -> VsOut {
    var out: VsOut;
    let corner = quad_corner(vid);
    let world_offset = inst.world_pos - u.viewport_center;
    let screen_center = world_offset * u.zoom;
    let pixel_pos = vec2<f32>(
        u.viewport_size.x * 0.5 + screen_center.x + corner.x * inst.world_size.x * u.zoom,
        u.viewport_size.y * 0.5 + screen_center.y + corner.y * inst.world_size.y * u.zoom,
    );
    let ndc = vec2<f32>(
        2.0 * pixel_pos.x / u.viewport_size.x - 1.0,
        1.0 - 2.0 * pixel_pos.y / u.viewport_size.y,
    );
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    // Map corner (-0.5..0.5) → 0..1 then into the atlas slice.
    let local_uv = corner + 0.5;
    out.uv = vec2<f32>(
        inst.uv_rect.x + local_uv.x * inst.uv_rect.z,
        inst.uv_rect.y + local_uv.y * inst.uv_rect.w,
    );
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let s = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    if s.a <= 0.001 {
        discard;
    }
    return s;
}
