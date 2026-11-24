import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::globals};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> opacity: f32;

// `WaterVolumes.tsx` wrote these straight to an sRGB canvas.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    let world = in.world_position.xyz;
    let ripple = sin(world.x * 5.0 + t * 1.9) * sin(world.z * 5.0 - t * 1.5);
    let glint = smoothstep(0.82, 1.0, ripple);
    let shallow = vec3(0.30, 0.80, 0.88);
    let deep = vec3(0.10, 0.55, 0.74);
    let uv = in.uv;
    let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    var colour = mix(shallow, deep, smoothstep(0.0, 0.35, edge));
    colour += glint * 0.35 + ripple * 0.03;
    let foam = 1.0 - smoothstep(0.0, 0.035, edge);
    colour = mix(colour, vec3(0.95, 1.0, 1.0), foam * 0.6);
    return vec4(srgb_to_linear(clamp(colour, vec3(0.0), vec3(1.0))), opacity);
}
