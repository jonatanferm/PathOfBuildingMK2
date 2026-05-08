// Arc-edge pipeline. Each instance describes an orbital connector — a
// short arc of constant radius `r` around a group `center`, swept from
// angle_a to angle_b. We tessellate the arc into SEGMENTS quads in the
// vertex shader so the result reads as a smooth curve at any zoom, and
// reuse the same colour logic as straight edges for visual consistency.
//
// Per-instance: center (vec2), radius (f32), angle_a (f32), angle_b (f32),
//               state (u32). angle_b is pre-normalised on the CPU side so
//               the linear interpolation between angle_a and angle_b walks
//               the *short* way around the circle (no wraparound seams).

struct Uniforms {
    viewport_center: vec2<f32>,
    zoom: f32,
    _pad0: f32,
    viewport_size: vec2<f32>,
    pixels_per_point: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct Arc {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) angle_a: f32,
    @location(3) angle_b: f32,
    @location(4) state: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) state: u32,
    @location(1) edge_n: f32, // -1..1 across the short axis (for AA)
};

const SEGMENTS: u32 = 16u;
const VERTS_PER_SEGMENT: u32 = 6u;

fn corner_offsets(idx: u32) -> vec2<f32> {
    // Two triangles forming a unit rectangle in (along, across).
    var lookup = array<vec2<f32>, 6>(
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

// Match `tree_layout::compute_node_positions`: y axis flipped so positive
// angle sweeps clockwise from north.
fn arc_point(center: vec2<f32>, radius: f32, angle: f32) -> vec2<f32> {
    return vec2<f32>(
        center.x + sin(angle) * radius,
        center.y - cos(angle) * radius,
    );
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, arc: Arc) -> VsOut {
    var out: VsOut;
    let segment_idx = vid / VERTS_PER_SEGMENT;
    let corner_idx = vid % VERTS_PER_SEGMENT;
    let off = corner_offsets(corner_idx);

    let t1 = f32(segment_idx) / f32(SEGMENTS);
    let t2 = f32(segment_idx + 1u) / f32(SEGMENTS);
    let angle1 = mix(arc.angle_a, arc.angle_b, t1);
    let angle2 = mix(arc.angle_a, arc.angle_b, t2);

    let p1_world = arc_point(arc.center, arc.radius, angle1);
    let p2_world = arc_point(arc.center, arc.radius, angle2);
    let p1_px = world_to_pixels(p1_world);
    let p2_px = world_to_pixels(p2_world);

    let dir = p2_px - p1_px;
    let len = max(length(dir), 0.0001);
    let tan = dir / len;
    let nor = vec2<f32>(-tan.y, tan.x);

    let allocated = (arc.state & 1u) != 0u;
    let path = (arc.state & 2u) != 0u;
    var half_w_px = select(0.6, 1.5, allocated);
    if path {
        half_w_px = 2.5;
    }

    let pos_px = p1_px + tan * (off.x * len) + nor * (off.y * half_w_px);
    let ndc = vec2<f32>(
        2.0 * pos_px.x / u.viewport_size.x - 1.0,
        1.0 - 2.0 * pos_px.y / u.viewport_size.y,
    );

    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.state = arc.state;
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
    let edge = 1.0 - smoothstep(0.6, 1.0, abs(in.edge_n));
    return vec4<f32>(color, alpha * edge);
}
