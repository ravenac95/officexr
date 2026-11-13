import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
};
import bevy_render::maths::PI;

struct HemisphereIrradiance {
    sky: vec4<f32>,
    ground: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> hemisphere: HemisphereIrradiance;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // three.js's hemisphere light: indirect irradiance through a Lambert BRDF.
    let irradiance = mix(hemisphere.ground.rgb, hemisphere.sky.rgb, 0.5 * pbr_input.N.y + 0.5);
    let diffuse = pbr_input.material.base_color.rgb * (1.0 - pbr_input.material.metallic);
    out.color += vec4(diffuse * irradiance / PI * view.exposure, 0.0);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
