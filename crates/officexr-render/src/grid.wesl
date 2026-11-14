import bevy_pbr::render::forward_io::VertexOutput;

struct Grid {
    cell_color: vec4<f32>,
    section_color: vec4<f32>,
    cell_size: f32,
    section_size: f32,
    fade_distance: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> grid: Grid;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

// One screen pixel wide at any zoom: the distance to the nearest line in
// cells, over how many cells a pixel spans.
fn grid_line(coord: vec2<f32>, scale: f32) -> f32 {
    let cells = coord / scale;
    let g = abs(fract(cells - 0.5) - 0.5) / fwidth(cells);
    return 1.0 - min(min(g.x, g.y), 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let xz = in.world_position.xz;
    let minor = grid_line(xz, grid.cell_size);
    let major = grid_line(xz, grid.section_size);
    let color = mix(grid.cell_color.rgb, grid.section_color.rgb, major);
    let fade = 1.0 - smoothstep(grid.fade_distance * 0.4, grid.fade_distance, length(xz));
    let a = max(minor, major) * fade;
    if a < 0.01 {
        discard;
    }
    // three.js's ShaderMaterial writes the linear colour with no encoding;
    // undo the sRGB target's encode so the same bytes land.
    return vec4(srgb_to_linear(color), a);
}
