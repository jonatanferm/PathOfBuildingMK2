// Instanced quad SDF circle pipeline for passive-tree nodes.
//
// Per-instance: world position (tree-space), world radius, kind index,
//                state byte (allocated|search-match|hovered).
// Uniforms: viewport_center (tree-space), zoom, viewport size in pixels.
// Output: full screen quad per node, with smooth circle SDF, ring stroke,
//         and an extra search-match outline.

struct Uniforms {
    viewport_center: vec2<f32>,
    zoom: f32,
    _pad0: f32,
    viewport_size: vec2<f32>, // pixels, before pixels-per-point
    pixels_per_point: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct Instance {
    @location(0) world_pos: vec2<f32>,
    @location(1) world_radius: f32,
    @location(2) kind: u32,
    @location(3) state: u32, // bit0: allocated, bit1: search-match, bit2: hovered, bit3: path
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>, // -1..1 within the quad
    @location(1) screen_radius: f32, // pixels
    @location(2) kind: u32,
    @location(3) state: u32,
};

// Generate a unit quad covering [-1, 1]^2 from vertex_index 0..6.
fn quad_corner(idx: u32) -> vec2<f32> {
    // Two triangles: (0,1,2), (0,2,3) where corners are
    // 0=(-1,-1), 1=(+1,-1), 2=(+1,+1), 3=(-1,+1)
    let lookup = array<vec2<f32>, 6>(
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

    // Pixel radius: world_radius * zoom. We give a couple of pixels of slack so
    // the AA edge has room. Apply a soft minimum so far-zoom-out nodes stay
    // visible.
    let raw_radius_px = inst.world_radius * u.zoom;
    let min_radius_px = 2.0;
    let radius_px = max(raw_radius_px, min_radius_px);
    // Extra slack for AA + search-match outer ring (3 px outward).
    let pad_px = 4.0;
    let half_quad_px = radius_px + pad_px;

    // Compute screen-space center in pixels from top-left.
    let world_offset = inst.world_pos - u.viewport_center;
    let screen_center = world_offset * u.zoom; // pixel offset from viewport centre

    // The viewport size from egui's set_viewport is in pixels. Convert to NDC:
    // x_ndc = 2 * (px / size_x) - 1, y_ndc = 1 - 2 * (py / size_y) (y flipped).
    let size_px = u.viewport_size; // already pixels
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
    // local maps -1..1 across the padded quad; the actual circle is at radius
    // (radius_px / half_quad_px). We pass local in pixel units relative to centre.
    out.local = corner * half_quad_px;
    out.screen_radius = radius_px;
    out.kind = inst.kind;
    out.state = inst.state;
    return out;
}

// Kind constants (mirror NodeKind enum). 0=Normal,1=Notable,2=Keystone,
// 3=Mastery,4=JewelSocket,5=Root,6=ClassStart,7=AscendancyStart,8=Tattoo,9=Blighted.
fn kind_color(kind: u32, allocated: bool) -> vec3<f32> {
    if allocated {
        switch kind {
            case 1u: { return vec3<f32>(0.86, 0.39, 0.39); } // Notable
            case 2u: { return vec3<f32>(0.86, 0.71, 0.24); } // Keystone
            case 3u: { return vec3<f32>(0.71, 0.31, 0.86); } // Mastery
            case 4u: { return vec3<f32>(0.24, 0.86, 0.86); } // JewelSocket
            default: { return vec3<f32>(0.39, 0.78, 1.00); } // Normal / starts
        }
    } else {
        switch kind {
            case 1u: { return vec3<f32>(0.55, 0.27, 0.27); } // Notable
            case 2u: { return vec3<f32>(0.47, 0.39, 0.20); } // Keystone
            case 3u: { return vec3<f32>(0.39, 0.24, 0.51); } // Mastery
            case 4u: { return vec3<f32>(0.16, 0.43, 0.43); } // JewelSocket
            case 6u: { return vec3<f32>(0.31, 0.31, 0.43); } // ClassStart
            case 7u: { return vec3<f32>(0.31, 0.31, 0.43); } // AscendancyStart
            default: { return vec3<f32>(0.27, 0.27, 0.35); } // Normal
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

    let fill = kind_color(in.kind, allocated);
    let ring_color = select(vec3<f32>(0.47, 0.47, 0.51), vec3<f32>(1.0, 1.0, 1.0), allocated);

    // 1 px AA edge.
    let aa = 1.0;
    let inside = 1.0 - smoothstep(r - aa, r, dist);
    let ring_w = 1.0;
    let ring = smoothstep(r - ring_w - aa, r - ring_w, dist) * (1.0 - smoothstep(r - aa, r, dist));

    var color = fill * inside + ring_color * ring;
    var alpha = max(inside, ring);

    // Search-match: yellow outer ring at r + 3.
    if search {
        let or = r + 3.0;
        let owidth = 2.0;
        let outer_ring = smoothstep(or - owidth - aa, or - owidth, dist)
                       * (1.0 - smoothstep(or - aa, or, dist));
        color = color * (1.0 - outer_ring) + vec3<f32>(1.0, 0.94, 0.31) * outer_ring;
        alpha = max(alpha, outer_ring);
    }
    // Hovered: brighter outer halo.
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
