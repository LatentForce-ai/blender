// 3D viewport floor grid and axes: a port of overlay_grid_vert.glsl and overlay_grid_frag.glsl
// (draw/engines/overlay/shaders/) for perspective views on the XY plane. One draw covers the
// axes (first AXIS_VERTEX_COUNT vertices) and all grid lines; each instance is one of Blender's
// depth-biased grid iterations.

const STEPS_LEN: i32 = 8;
const STEPS_DRAW: u32 = 3u;
const ITER_LEN: u32 = 4u;
const AXIS_VERTEX_COUNT: u32 = 6u;

// `OVERLAY_GridBits`.
const SHOW_GRID: u32 = 1u;
const SHOW_AXES: u32 = 2u;
const AXIS_X: u32 = 4u;
const AXIS_Y: u32 = 8u;
const AXIS_Z: u32 = 16u;
const PLANE_XY: u32 = 32u;

struct GridData {
    view_projection: mat4x4<f32>,
    // xyz: camera position; w: far clip distance.
    view_position: vec4<f32>,
    steps: array<vec4<f32>, 8>,
    color_grid: vec4<f32>,
    color_emphasis: vec4<f32>,
    color_axes: array<vec4<f32>, 3>,
    offset: vec2<f32>,
    clip_rect: vec2<f32>,
    level: f32,
    num_lines: u32,
    grid_flag: u32,
    axis_flag: u32,
    viewport_size: vec2<f32>,
}

@group(0) @binding(0) var<uniform> grid: GridData;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) pos: vec3<f32>,
    @location(1) coord: vec2<f32>,
    @location(2) @interpolate(flat) alpha: f32,
    @location(3) @interpolate(flat) emphasis: f32,
    @location(4) @interpolate(flat) edge_start: vec2<f32>,
    // Blender interpolates this without perspective; WebGL2 has no `noperspective`, and the
    // difference only shifts the low-alpha dash pattern slightly.
    @location(5) edge_pos: vec2<f32>,
    @location(6) @interpolate(flat) flag: u32,
    @location(7) @interpolate(flat) iteration: u32,
}

struct LineData {
    p: vec2<f32>,
    axis: u32,
    level: u32,
}

fn has(flag: u32, bits: u32) -> bool {
    return (flag & bits) != 0u;
}

fn decode_grid_data(id: u32) -> LineData {
    var vertex_id = id;
    var line: LineData;
    // Each pair of vertices is a line; each pair of lines flips between X and Y.
    let side = vertex_id & 1u;
    vertex_id = vertex_id >> 1u;
    line.axis = vertex_id & 1u;
    vertex_id = vertex_id >> 1u;
    // Remaining bits: level (large to small) and index of the line.
    line.level = STEPS_DRAW - 1u - vertex_id / grid.num_lines;
    vertex_id = vertex_id % grid.num_lines;
    let lines_half = max(f32(grid.num_lines >> 1u), 1.0);
    let line_index = (f32(vertex_id) - lines_half) / lines_half;
    line.p = vec2(line_index, select(-1.0, 1.0, side != 0u));
    if (line.axis != 0u) {
        line.p = line.p.yx;
    }
    return line;
}

fn decode_axis_data(vertex_id: u32) -> LineData {
    var line: LineData;
    line.axis = vertex_id >> 1u;
    line.level = STEPS_DRAW - 1u;
    line.p = vec2(select(-1.0, 1.0, (vertex_id & 1u) != 0u), 0.0);
    return line;
}

// Whether a line of a coarser level lies on top of this one (i * s1 == j * s2 for integer j).
fn is_occluded_by_higher_level(line: LineData, level: i32, flag: u32) -> bool {
    if (!has(flag, SHOW_GRID) || line.level >= STEPS_DRAW - 1u || level >= STEPS_LEN - 1) {
        return false;
    }
    let j = abs(line.p[line.axis]) / grid.steps[level + 1][line.axis];
    return abs(floor(j) - j) <= 1e-4;
}

fn screen_position(p: vec4<f32>) -> vec2<f32> {
    return ((p.xy / p.w) * 0.5 + 0.5) * grid.viewport_size;
}

// Outside the clip volume, so the whole line is dropped.
const DISCARDED = vec4(2.0, 2.0, 2.0, 1.0);
// Blender's z-bias range for GL's [-1, 1] clip depth, halved for WebGPU's [0, 1].
const Z_MIN_OFFSET: f32 = -0.00020 * 0.5;
const Z_MAX_OFFSET: f32 = 0.00025 * 0.5;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) iteration: u32) -> VertexOut {
    var out: VertexOut;
    out.position = DISCARDED;
    let is_axis = vertex_index < AXIS_VERTEX_COUNT;
    let flag = select(grid.grid_flag, grid.axis_flag, is_axis);
    out.flag = flag;
    out.iteration = iteration;
    if (flag == 0u) {
        return out;
    }
    var line: LineData;
    if (is_axis) {
        line = decode_axis_data(vertex_index);
    } else {
        line = decode_grid_data(vertex_index - AXIS_VERTEX_COUNT);
    }

    // Level of this line, offset by -1 to force a sub-level in the 3D viewport.
    let level = clamp(i32(grid.level) + i32(line.level) - 1, 0, STEPS_LEN - 1);
    let step_size = grid.steps[level][line.axis];
    if (step_size == 0.0) {
        return out;
    }

    // Grid lines snap to their level's spacing along the other axis; axes follow the camera.
    var step_offs = grid.offset;
    if (has(flag, SHOW_GRID)) {
        if (line.axis != 0u) {
            step_offs.y = round(step_offs.y / step_size) * step_size;
        } else {
            step_offs.x = round(step_offs.x / step_size) * step_size;
        }
    } else {
        step_offs = vec2(grid.view_position[line.axis], 0.0);
    }

    out.coord = line.p;
    out.emphasis = saturate(f32(line.level) - fract(grid.level));
    out.alpha = saturate(f32(line.level) + 1.0 - fract(grid.level));

    line.p = step_offs + step_size * line.p * max(f32(grid.num_lines >> 1u), 1.0);

    var clip_min = step_offs - grid.clip_rect;
    var clip_max = step_offs + grid.clip_rect;
    if (!has(flag, SHOW_GRID)) {
        // Both clip components are the far clip distance, so every axis uses `x`.
        clip_min = vec2(step_offs.x - grid.clip_rect.x, 0.0);
        clip_max = vec2(step_offs.x + grid.clip_rect.x, 0.0);
    }
    if (all(line.p < clip_min) || all(line.p > clip_max)) {
        return out;
    }
    line.p = clamp(line.p, clip_min, clip_max);

    var pos = vec3(0.0);
    if (has(flag, SHOW_GRID)) {
        pos = vec3(line.p, 0.0);
    } else {
        // AXIS_X, AXIS_Y and AXIS_Z are consecutive bits.
        if (!has(flag, AXIS_X << line.axis)) {
            return out;
        }
        pos[line.axis] = line.p.x;
    }
    if (is_occluded_by_higher_level(line, level, flag)) {
        return out;
    }

    out.pos = pos;
    out.position = grid.view_projection * vec4(pos, 1.0);
    // Each iteration sits at a slightly different depth so the grid fades through geometry.
    let z_factor = f32(iteration * STEPS_DRAW + line.level) / f32(ITER_LEN * STEPS_DRAW);
    out.position.z += mix(Z_MAX_OFFSET, Z_MIN_OFFSET, z_factor);
    out.edge_start = screen_position(out.position);
    out.edge_pos = out.edge_start;
    return out;
}

const AXIS_EPSILON: f32 = 2e-7;
// Width of the dash pattern for faint lines, in pixels.
const DASH_WIDTH: f32 = 4.0;
// Alpha below which faint lines turn into dashes.
const FADE_START: f32 = 0.1;

fn pow3(x: f32) -> f32 {
    return x * x * x;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let on_axis_x = has(in.flag, AXIS_X) && all(abs(in.pos.yz) <= vec2(AXIS_EPSILON));
    let on_axis_y = has(in.flag, AXIS_Y) && all(abs(in.pos.xz) <= vec2(AXIS_EPSILON));
    let on_axis_z = has(in.flag, AXIS_Z) && all(abs(in.pos.xy) <= vec2(AXIS_EPSILON));
    let on_axis = on_axis_x || on_axis_y || on_axis_z;
    // Grid lines under an axis line give way to it.
    if (on_axis && has(in.flag, SHOW_GRID)) {
        discard;
    }

    var color = mix(grid.color_grid, grid.color_emphasis, in.emphasis);
    if (on_axis_x) {
        color = grid.color_axes[0];
    } else if (on_axis_y) {
        color = grid.color_axes[1];
    } else if (on_axis_z) {
        color = grid.color_axes[2];
    }
    color.a *= in.alpha;

    // Fade toward the edge of the grid level.
    color.a *= 1.0 - min(1.0, length(in.coord));
    let to_camera = grid.view_position.xyz - in.pos;
    let dist = length(to_camera);
    // Fade the floor at grazing angles.
    if (in.pos.z == 0.0) {
        color.a *= 1.0 - pow3(1.0 - abs(to_camera.z / dist));
    }
    // Fade toward the far clip plane.
    let far_clip = grid.view_position.w;
    color.a *= 1.0 - smoothstep(0.0, 0.5 * far_clip, dist - 0.5 * far_clip);

    // Faint lines become dashes instead of disappearing into a haze.
    let dash = fract(distance(in.edge_start, in.edge_pos) / DASH_WIDTH);
    if (color.a < FADE_START && color.a / FADE_START < dash) {
        discard;
    }
    // Later iterations add less, so they don't pop in: 1, 1/2, 1/4, 1/8.
    color.a *= exp2(-f32(in.iteration));
    return color;
}
