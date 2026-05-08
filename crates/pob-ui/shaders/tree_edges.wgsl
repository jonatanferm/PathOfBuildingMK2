// Edge pipeline. We draw thin quads (six vertices per edge instance) that span
// from world-space `a` to world-space `b`, fattened in pixel space so the edge
// remains visible at any zoom.

struct Uniforms {
    viewport_center: vec2<f32>,
    zoom: f32,
    _pad0: f32,
    viewport_size: vec2<f32>,
    pixels_per_point: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct Edge {
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) state: u32, // bit0: both allocated, bit1: path overlay
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) state: u32,
    @location(1) edge_t: f32, // 0..1 for AA along long axis (unused)
    @location(2) edge_n: f32, // -1..1 across the short axis
};

fn corner_offsets(idx: u32) -> vec2<f32> {
    // Two triangles forming a unit rectangle in (along, across).
    // along = 0 at A, 1 at B; across = -1 to 1.
    let lookup = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0,  1.0),
        vec2<f32>(0.0, -1.0),
        vec2<f32>(1.0,  1.0),
        vec2<f32>(0.0,  1.0),
    );
    return lookup[idx];
}

fn world_to_pixels(p: vec2<f32>) -> vec2<f32> {
    let world_offset = p - u.viewport_center;
    let screen_offset = world_offset * u.zoom;
    return vec2<f32>(
        u.viewport_size.x * 0.5 + screen_offset.x,
        u.viewport_size.y * 0.5 + screen_offset.y,
    );
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, edge: Edge) -> VsOut {
    var out: VsOut;
    let off = corner_offsets(vid);

    let a_px = world_to_pixels(edge.a);
    let b_px = world_to_pixels(edge.b);

    let dir = b_px - a_px;
    let len = max(length(dir), 0.0001);
    let tan = dir / len;
    let nor = vec2<f32>(-tan.y, tan.x);

    let allocated = (edge.state & 1u) != 0u;
    let path = (edge.state & 2u) != 0u;
    var half_w_px = select(0.6, 1.5, allocated);
    if path {
        half_w_px = 2.5;
    }

    let pos_px = a_px + tan * (off.x * len) + nor * (off.y * half_w_px);
    let ndc = vec2<f32>(
        2.0 * pos_px.x / u.viewport_size.x - 1.0,
        1.0 - 2.0 * pos_px.y / u.viewport_size.y,
    );

    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.state = edge.state;
    out.edge_t = off.x;
    out.edge_n = off.y;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let allocated = (in.state & 1u) != 0u;
    let path = (in.state & 2u) != 0u;
    var color = select(vec3<f32>(0.27, 0.27, 0.31), vec3<f32>(0.71, 0.86, 1.0), allocated);
    var alpha = 0.95;
    if path {
        color = vec3<f32>(1.0, 0.78, 0.31);
        alpha = 1.0;
    }
    // Soft AA along the cross axis.
    let edge = 1.0 - smoothstep(0.6, 1.0, abs(in.edge_n));
    return vec4<f32>(color, alpha * edge);
}
