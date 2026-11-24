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

struct AvatarLook {
    tint: vec4<f32>,
    glow: f32,
    desaturate: f32,
    rim: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> hemisphere: HemisphereIrradiance;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> look: AvatarLook;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    // `Adventurer.tsx`'s spell tint: the colour factor lerped toward the tint.
    let base = pbr_input.material.base_color.rgb * mix(vec3(1.0), look.tint.rgb, look.tint.a);
    // `frozenLook.ts`: mostly grey.
    let grey = dot(base, vec3(0.299, 0.587, 0.114));
    pbr_input.material.base_color = vec4(mix(base, vec3(grey), look.desaturate), pbr_input.material.base_color.a);
    let facing = saturate(dot(normalize(pbr_input.N), normalize(pbr_input.V)));
    let rim = look.rim.rgb * look.rim.a * pow(1.0 - facing, 2.5);
    let glow = look.tint.rgb * look.tint.a * look.glow;
    pbr_input.material.emissive = vec4(pbr_input.material.emissive.rgb + rim + glow, pbr_input.material.emissive.a);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // three.js's hemisphere light: indirect irradiance through a Lambert BRDF.
    let irradiance = mix(hemisphere.ground.rgb, hemisphere.sky.rgb, 0.5 * pbr_input.N.y + 0.5);
    let diffuse = pbr_input.material.base_color.rgb * (1.0 - pbr_input.material.metallic);
    out.color += vec4(diffuse * irradiance / PI * view.exposure, 0.0);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
