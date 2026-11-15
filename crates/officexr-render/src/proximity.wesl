import bevy_pbr::render::forward_io::VertexOutput;

// `color` is premultiplied by its opacity; alpha 0 makes the premultiplied blend additive.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> color: vec4<f32>;

// `radialTexture()`'s gradient: alpha 1 at the centre, 0.4 halfway, 0 at the rim.
fn falloff(r: f32) -> f32 {
    if r < 0.5 {
        return mix(1.0, 0.4, r / 0.5);
    }
    return mix(0.4, 0.0, min((r - 0.5) / 0.5, 1.0));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = length(in.uv - vec2(0.5)) * 2.0;
    return vec4(color.rgb * falloff(r), 0.0);
}
