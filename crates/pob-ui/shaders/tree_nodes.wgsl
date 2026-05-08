// Instanced quad SDF circle pipeline for passive-tree nodes, with optional
// per-instance icon sampled from the skills atlas.
//
// Per-instance: world position (tree-space), world radius, kind index,
//                state byte, atlas UV rect (u, v, du, dv) in [0, 1].
//                When the rect is all zeros the node renders as a flat
//                colored circle (used for nodes without an icon).
// Uniforms: viewport_center (tree-space), zoom, viewport size in pixels.
// Output: full screen quad per node, with smooth circle SDF, ring stroke,
//         optional atlas sample, and an extra search-match outline.

struct Uniforms {
    viewport_center: vec2<f32>,
    zoom: f32,
    _pad0: f32,
    viewport_size: vec2<f32>,
    pixels_per_point: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas_active: texture_2d<f32>;
@group(0) @binding(2) var atlas_inactive: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;
// Per-mastery-group icons live in a separate atlas (`mastery-3.png`)
// because their pixel size and naming convention differ from the
// normal/notable/keystone shared atlas.
@group(0) @binding(4) var atlas_mastery: texture_2d<f32>;

struct Instance {
    @location(0) world_pos: vec2<f32>,
    @location(1) world_radius: f32,
    @location(2) kind: u32,
    @location(3) state: u32, // bit0: allocated, bit1: search-match, bit2: hovered, bit3: path
    @location(4) icon_uv: vec4<f32>, // x, y, w, h normalised in atlas
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>, // pixel offset from quad centre
    @location(1) screen_radius: f32, // pixels
    @location(2) kind: u32,
    @location(3) state: u32,
    @location(4) icon_uv: vec4<f32>,
};

// Generate a unit quad covering [-1, 1]^2 from vertex_index 0..6.
fn quad_corner(idx: u32) -> vec2<f32> {
    var lookup = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    return lookup[idx];
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    inst: Instance,
) -> VsOut {
    var out: VsOut;
    let corner = quad_corner(vid);

    let raw_radius_px = inst.world_radius * u.zoom;
    let min_radius_px = 2.0;
    let radius_px = max(raw_radius_px, min_radius_px);
    let pad_px = 4.0;
    let half_quad_px = radius_px + pad_px;

    let world_offset = inst.world_pos - u.viewport_center;
    let screen_center = world_offset * u.zoom;

    let size_px = u.viewport_size;
    let quad_offset_px = corner * half_quad_px;
    let pixel_pos = vec2<f32>(
        size_px.x * 0.5 + screen_center.x + quad_offset_px.x,
        size_px.y * 0.5 + screen_center.y + quad_offset_px.y,
    );
    let ndc = vec2<f32>(
        2.0 * pixel_pos.x / size_px.x - 1.0,
        1.0 - 2.0 * pixel_pos.y / size_px.y,
    );
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.local = corner * half_quad_px;
    out.screen_radius = radius_px;
    out.kind = inst.kind;
    out.state = inst.state;
    out.icon_uv = inst.icon_uv;
    return out;
}

// Kind constants (mirror NodeKind enum). 0=Normal,1=Notable,2=Keystone,
// 3=Mastery,4=JewelSocket,5=Root,6=ClassStart,7=AscendancyStart,8=Tattoo,9=Blighted.
fn kind_color(kind: u32, allocated: bool) -> vec3<f32> {
    if allocated {
        switch kind {
            case 1u: { return vec3<f32>(0.86, 0.39, 0.39); }
            case 2u: { return vec3<f32>(0.86, 0.71, 0.24); }
            case 3u: { return vec3<f32>(0.71, 0.31, 0.86); }
            case 4u: { return vec3<f32>(0.24, 0.86, 0.86); }
            default: { return vec3<f32>(0.39, 0.78, 1.00); }
        }
    } else {
        switch kind {
            case 1u: { return vec3<f32>(0.55, 0.27, 0.27); }
            case 2u: { return vec3<f32>(0.47, 0.39, 0.20); }
            case 3u: { return vec3<f32>(0.39, 0.24, 0.51); }
            case 4u: { return vec3<f32>(0.16, 0.43, 0.43); }
            case 6u: { return vec3<f32>(0.31, 0.31, 0.43); }
            case 7u: { return vec3<f32>(0.31, 0.31, 0.43); }
            default: { return vec3<f32>(0.27, 0.27, 0.35); }
        }
    }
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dist = length(in.local);
    let r = in.screen_radius;
    let allocated = (in.state & 1u) != 0u;
    let search    = (in.state & 2u) != 0u;
    let hovered   = (in.state & 4u) != 0u;

    let aa = 1.0;
    let inside = 1.0 - smoothstep(r - aa, r, dist);
    let ring_w = 1.0;
    let ring = smoothstep(r - ring_w - aa, r - ring_w, dist) * (1.0 - smoothstep(r - aa, r, dist));

    let has_icon = in.icon_uv.z > 0.0 && in.icon_uv.w > 0.0;
    var fill = kind_color(in.kind, allocated);

    if has_icon {
        // Map the quad's local pixel offset (-half..+half) into the atlas
        // sub-rect for this instance. The padded quad extends past r by a
        // few pixels (for AA + outer rings), so local_uv ranges from
        // ~-0.25..1.25; clamping to 0..1 keeps the atlas sample inside the
        // icon's rect rather than bleeding into neighbouring atlas cells
        // (matters most for the mastery atlas which is sparse).
        let local_uv = clamp(in.local / r * 0.5 + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
        let atlas_uv = vec2<f32>(
            in.icon_uv.x + local_uv.x * in.icon_uv.z,
            in.icon_uv.y + local_uv.y * in.icon_uv.w,
        );
        // Sample all three atlases unconditionally and pick afterwards —
        // some naga / browser-WebGPU paths choke on conditional texture
        // reads inside an `if` (uniformity analysis treats them as
        // non-uniform even when in.kind is constant per draw).
        let s_active = textureSampleLevel(atlas_active, atlas_sampler, atlas_uv, 0.0);
        let s_inactive = textureSampleLevel(atlas_inactive, atlas_sampler, atlas_uv, 0.0);
        let s_mastery = textureSampleLevel(atlas_mastery, atlas_sampler, atlas_uv, 0.0);
        var sampled: vec4<f32>;
        if in.kind == 3u {
            sampled = s_mastery;
        } else {
            sampled = select(s_inactive, s_active, allocated);
        }
        // Sit the sampled icon over the kind-color tint, weighted by the
        // sampled alpha so transparent atlas pixels (mastery icons in
        // particular) don't darken the underlying fill.
        let icon_alpha = sampled.a;
        fill = mix(fill, sampled.rgb, icon_alpha * 0.85);
    }

    let ring_color = select(vec3<f32>(0.47, 0.47, 0.51), vec3<f32>(1.0, 1.0, 1.0), allocated);

    var color = fill * inside + ring_color * ring;
    var alpha = max(inside, ring);

    if search {
        let or = r + 3.0;
        let owidth = 2.0;
        let outer_ring = smoothstep(or - owidth - aa, or - owidth, dist)
                       * (1.0 - smoothstep(or - aa, or, dist));
        color = color * (1.0 - outer_ring) + vec3<f32>(1.0, 0.94, 0.31) * outer_ring;
        alpha = max(alpha, outer_ring);
    }
    if hovered {
        let hr = r + 2.0;
        let hwidth = 1.5;
        let halo = smoothstep(hr - hwidth - aa, hr - hwidth, dist)
                 * (1.0 - smoothstep(hr - aa, hr, dist));
        color = color * (1.0 - halo) + vec3<f32>(1.0, 1.0, 1.0) * halo;
        alpha = max(alpha, halo);
    }

    if alpha <= 0.001 {
        discard;
    }
    return vec4<f32>(color, alpha);
}
