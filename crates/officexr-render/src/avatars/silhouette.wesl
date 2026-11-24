import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct Silhouette {
    color: vec4<f32>,
    /// Metres toward the camera the depth test is done from.
    reach: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> silhouette: Silhouette;

struct Out {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// Drawn where the depth test fails for a point `reach` nearer the camera than
// the body: so only where something at least that far in front hides it,
// never where the body's own limbs cross it.
@fragment
fn fragment(in: VertexOutput) -> Out {
    let toward_camera = normalize(view.world_position - in.world_position.xyz);
    let nearer = in.world_position.xyz + toward_camera * silhouette.reach;
    let clip = view.clip_from_world * vec4(nearer, 1.0);
    var out: Out;
    out.color = silhouette.color;
    out.depth = clip.z / clip.w;
    return out;
}
