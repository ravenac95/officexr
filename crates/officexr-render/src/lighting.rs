//! Port of `LightingRig.tsx` and `shadow-snap.ts`.

use std::f32::consts::PI;

use bevy::color::{LinearRgba, Srgba};
use bevy::light::cascade::Cascade;
use bevy::light::{
    CascadeShadowConfig, Cascades, DirectionalLight, DirectionalLightShadowMap, PointLight,
    SimulationLightSystems, SpotLight,
};
use bevy::math::{Mat3, Mat4, Quat, Vec3, Vec4};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, StandardMaterial};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::scene::{AuxLightType, LightingConfig, WorldSceneData};
use crate::{CAMERA_FAR, SceneCamera, SceneEntity};

const HEMISPHERE_SKY: [u8; 3] = [0xae, 0xdc, 0xff];
const HEMISPHERE_GROUND: [u8; 3] = [0x3a, 0x2f, 0x24];
const SHADOW_NEAR: f32 = 1.0;

/// `StandardMaterial` plus three.js's hemisphere and ambient lights, which
/// Bevy has no equivalent for (its ambient adds a specular term).
pub type ObjectMaterial = ExtendedMaterial<StandardMaterial, HemisphereFill>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct HemisphereFill {
    #[uniform(100)]
    pub irradiance: HemisphereIrradiance,
}

#[derive(ShaderType, Reflect, Debug, Clone, Copy, Default)]
pub struct HemisphereIrradiance {
    pub sky: LinearRgba,
    pub ground: LinearRgba,
}

impl MaterialExtension for HemisphereFill {
    fn fragment_shader() -> ShaderRef {
        "embedded://officexr_render/hemisphere.wesl".into()
    }
}

impl HemisphereFill {
    pub fn new(lighting: &LightingConfig) -> Self {
        let fill = LinearRgba::WHITE * lighting.ambient_fill_intensity;
        let sky = hemisphere_color(HEMISPHERE_SKY) * lighting.ambient_intensity + fill;
        let ground = hemisphere_color(HEMISPHERE_GROUND) * lighting.ambient_intensity + fill;
        Self {
            irradiance: HemisphereIrradiance { sky, ground },
        }
    }
}

fn hemisphere_color([r, g, b]: [u8; 3]) -> LinearRgba {
    Srgba::rgb_u8(r, g, b).into()
}

#[derive(Component)]
pub(crate) struct Sun;

pub(crate) fn spawn_lights(mut commands: Commands, scene: Res<WorldSceneData>) {
    let l = &scene.lighting;
    let sun = Vec3::from(l.sun_position);
    let at_sun = Transform::from_translation(sun).looking_at(Vec3::ZERO, Vec3::Y);
    commands.insert_resource(DirectionalLightShadowMap {
        size: l.shadow_map_size as usize,
    });
    let texel = shadow_texel_size(l);
    commands.spawn((
        SceneEntity,
        Sun,
        DirectionalLight {
            color: l.sun_color,
            illuminance: l.sun_intensity,
            shadow_maps_enabled: l.cast_shadow,
            shadow_depth_bias: -l.shadow_bias * (shadow_far(l) - SHADOW_NEAR),
            shadow_normal_bias: l.shadow_normal_bias / texel,
            ..default()
        },
        // Only the first cascade's far bound matters: `fit_sun_shadow`
        // replaces the cascades themselves.
        CascadeShadowConfig {
            bounds: vec![CAMERA_FAR],
            overlap_proportion: 0.0,
            minimum_distance: 0.0,
        },
        at_sun,
    ));

    // three.js lights are in candela; Bevy's point and spot lights in lumens.
    let lumens = l.aux_intensity * 4.0 * PI;
    let range = if l.aux_distance > 0.0 {
        l.aux_distance
    } else {
        f32::MAX
    };
    // ponytail: Bevy's range falloff is fixed inverse-square; `auxDecay` other than 2 is ignored.
    match l.aux_light_type {
        AuxLightType::None => {}
        AuxLightType::Point => {
            commands.spawn((
                SceneEntity,
                PointLight {
                    color: l.sun_color,
                    intensity: lumens,
                    range,
                    ..default()
                },
                at_sun,
            ));
        }
        AuxLightType::Spot => {
            commands.spawn((
                SceneEntity,
                SpotLight {
                    color: l.sun_color,
                    intensity: lumens,
                    range,
                    outer_angle: l.aux_angle,
                    inner_angle: l.aux_angle * (1.0 - l.aux_penumbra),
                    ..default()
                },
                at_sun,
            ));
        }
    }
}

fn shadow_far(l: &LightingConfig) -> f32 {
    Vec3::from(l.sun_position).length() + l.shadow_range + 20.0
}

fn shadow_texel_size(l: &LightingConfig) -> f32 {
    2.0 * l.shadow_range / l.shadow_map_size as f32
}

/// Quantises `point` to whole shadow-map texels along the light's right/up
/// axes (the columns of `basis`); the depth component is unchanged.
pub fn snap_to_shadow_texel(point: Vec3, basis: Mat3, texel_size: f32) -> Vec3 {
    let snap = |axis: Vec3| (point.dot(axis) / texel_size).round() * texel_size;
    basis.x_axis * snap(basis.x_axis)
        + basis.y_axis * snap(basis.y_axis)
        + basis.z_axis * point.dot(basis.z_axis)
}

/// Replaces Bevy's view-fitted cascades with three.js's single fixed
/// orthographic shadow camera: `shadowRange` around the texel-snapped focus,
/// from the sun out to `|sunPosition| + shadowRange + 20`.
// ponytail: the box centres on the camera look_at; TS uses the light target (origin, or the player in gameplay).
fn fit_sun_shadow(
    scene: Res<WorldSceneData>,
    camera: Res<SceneCamera>,
    mut suns: Query<(&Transform, &mut Cascades), With<Sun>>,
) {
    let l = &scene.lighting;
    for (transform, mut cascades) in &mut suns {
        let cascade = sun_cascade(transform.rotation, Vec3::from(camera.look_at), l);
        for view in cascades.cascades.values_mut() {
            *view = vec![cascade.clone()];
        }
    }
}

fn sun_cascade(rotation: Quat, focus: Vec3, l: &LightingConfig) -> Cascade {
    let texel_size = shadow_texel_size(l);
    let world_from_light = Mat4::from_quat(rotation);
    let snapped = snap_to_shadow_texel(focus, Mat3::from_quat(rotation), texel_size);
    let light_from_world = world_from_light.transpose();
    let near_plane_centre = light_from_world.transform_point3(snapped + Vec3::from(l.sun_position))
        - Vec3::Z * SHADOW_NEAR;
    let cascade_from_world = Mat4::from_cols(
        light_from_world.x_axis,
        light_from_world.y_axis,
        light_from_world.z_axis,
        (-near_plane_centre).extend(1.0),
    );
    let world_from_cascade = Mat4::from_cols(
        world_from_light.x_axis,
        world_from_light.y_axis,
        world_from_light.z_axis,
        world_from_light * near_plane_centre.extend(1.0),
    );
    // Reverse-Z orthographic, as Bevy's own cascades.
    let half = l.shadow_range;
    let clip_from_cascade = Mat4::from_cols(
        Vec4::new(1.0 / half, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0 / half, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0 / (shadow_far(l) - SHADOW_NEAR), 0.0),
        Vec4::new(0.0, 0.0, 1.0, 1.0),
    );
    Cascade {
        world_from_cascade,
        clip_from_cascade,
        clip_from_world: clip_from_cascade * cascade_from_world,
        texel_size,
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        fit_sun_shadow
            .after(SimulationLightSystems::UpdateDirectionalLightCascades)
            .before(SimulationLightSystems::UpdateLightFrusta)
            .run_if(resource_exists::<WorldSceneData>)
            .run_if(resource_exists::<SceneCamera>),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basis() -> Mat3 {
        Mat3::from_quat(
            Transform::from_xyz(20.0, 40.0, 20.0)
                .looking_at(Vec3::ZERO, Vec3::Y)
                .rotation,
        )
    }

    #[test]
    fn snapping_lands_on_the_texel_grid_and_keeps_depth() {
        let b = basis();
        let p = Vec3::new(3.3, 1.7, -8.2);
        let s = snap_to_shadow_texel(p, b, 0.25);
        for axis in [b.x_axis, b.y_axis] {
            let t = s.dot(axis) / 0.25;
            assert!((t - t.round()).abs() < 1e-4);
        }
        assert!((s.dot(b.z_axis) - p.dot(b.z_axis)).abs() < 1e-5);
    }

    #[test]
    fn snapping_is_stable_within_a_texel() {
        let b = basis();
        let a = snap_to_shadow_texel(Vec3::new(1.0, 0.0, 1.0), b, 1.0);
        let c = snap_to_shadow_texel(Vec3::new(1.0, 0.0, 1.0) + b.x_axis * 0.1, b, 1.0);
        assert!((a - c).length() < 1e-5);
    }
}
