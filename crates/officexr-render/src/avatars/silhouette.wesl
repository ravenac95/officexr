import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct Silhouette {
    color: vec4<f32>,
    /// The avatar's capsule: its axis from `base` to `top`, and its radius.
    base: vec3<f32>,
    radius: f32,
    top: vec3<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> silhouette: Silhouette;

struct Out {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

/// Distance along `rd` from `ro` to where the ray enters the capsule, or -1.
fn capsule_entry(ro: vec3<f32>, rd: vec3<f32>, pa: vec3<f32>, pb: vec3<f32>, r: f32) -> f32 {
    let ba = pb - pa;
    let oa = ro - pa;
    let baba = dot(ba, ba);
    let bard = dot(ba, rd);
    let baoa = dot(ba, oa);
    let rdoa = dot(rd, oa);
    let oaoa = dot(oa, oa);
    let a = baba - bard * bard;
    let b = baba * rdoa - baoa * bard;
    let c = baba * oaoa - baoa * baoa - r * r * baba;
    let h = b * b - a * c;
    if h < 0.0 {
        return -1.0;
    }
    // Where along the axis the ray meets the side; straight down it, which end the eye is past.
    var y = baoa;
    if a > 1e-6 {
        let t = (-b - sqrt(h)) / a;
        y = baoa + t * bard;
        if y > 0.0 && y < baba {
            return t;
        }
    }
    // The ray enters through an end cap.
    let oc = select(ro - pb, oa, y <= 0.0);
    let b2 = dot(rd, oc);
    let h2 = b2 * b2 - (dot(oc, oc) - r * r);
    if h2 < 0.0 {
        return -1.0;
    }
    return -b2 - sqrt(h2);
}

// Drawn where the depth test fails for the point the view ray enters the
// avatar's capsule: so only where something outside the avatar hides it,
// never where its own head or limbs cross its body.
@fragment
fn fragment(in: VertexOutput) -> Out {
    let eye = view.world_position;
    let to_fragment = in.world_position.xyz - eye;
    let dist = length(to_fragment);
    let rd = to_fragment / dist;
    let entry = capsule_entry(eye, rd, silhouette.base, silhouette.top, silhouette.radius);
    // Outside the capsule (a held prop sticking out) or with the eye inside
    // it: test from just in front of the fragment itself.
    let t = select(dist - 0.05, min(entry, dist - 0.05), entry > 0.0);
    let clip = view.clip_from_world * vec4(eye + rd * t, 1.0);
    var out: Out;
    out.color = silhouette.color;
    out.depth = clip.z / clip.w;
    return out;
}
