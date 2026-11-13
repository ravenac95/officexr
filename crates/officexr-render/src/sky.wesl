import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct Sky {
    top: vec4<f32>,
    horizon: vec4<f32>,
    bottom: vec4<f32>,
    // xyz: unit direction to the sun; w: the disc's angular radius.
    sun_dir: vec4<f32>,
    // rgb: sun colour; a: glow.
    sun_color: vec4<f32>,
    has_horizon: u32,
    show_sun: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sky: Sky;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position);
    let y = dir.y;
    var col: vec3<f32>;
    if sky.has_horizon == 0u {
        col = mix(sky.bottom.rgb, sky.top.rgb, smoothstep(-0.2, 1.0, y));
    } else if y >= 0.0 {
        col = mix(sky.horizon.rgb, sky.top.rgb, pow(smoothstep(0.0, 1.0, y), 0.6));
    } else {
        col = mix(sky.horizon.rgb, sky.bottom.rgb, smoothstep(0.0, 0.35, -y));
    }

    if sky.show_sun != 0u {
        let sun_dir = sky.sun_dir.xyz;
        let sun_color = sky.sun_color.rgb;
        let glow = sky.sun_color.a;
        let x = acos(clamp(dot(dir, sun_dir), -1.0, 1.0)) / sky.sun_dir.w;

        let dir_xz = dir.xz;
        let sun_xz = sun_dir.xz;
        if dot(dir_xz, dir_xz) > 1e-6 && dot(sun_xz, sun_xz) > 1e-6 {
            let facing = max(dot(normalize(dir_xz), normalize(sun_xz)), 0.0);
            col += sun_color * 0.18 * glow * pow(facing, 4.0) * pow(1.0 - abs(y), 8.0);
        }

        let warm = mix(sun_color, vec3(1.0), 0.5);
        let outside = max(x - 1.0, 0.0);
        col += sun_color * glow * (0.06 * exp(-x / 12.0) + 0.12 * exp(-x / 3.0));
        col += warm * glow * 0.45 * exp(-outside * 2.8);

        let core = 1.0 - smoothstep(0.8, 1.1, x);
        col = mix(col, vec3(1.0, 0.995, 0.97), core);
    }

    // three.js writes the linear gradient straight to the canvas with no
    // encoding; undo the sRGB target's encode so the same bytes land.
    return vec4(srgb_to_linear(min(col, vec3(1.0))), 1.0);
}
