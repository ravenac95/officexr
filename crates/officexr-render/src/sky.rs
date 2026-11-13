//! Port of `SkyDome.tsx`.

use bevy::color::LinearRgba;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::Vec3;
use bevy::mesh::{Mesh, Mesh3d, MeshVertexBufferLayoutRef, SphereMeshBuilder};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MeshMaterial3d};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::scene::{BackgroundConfig, LightingConfig};

const SKY_RADIUS: f32 = 1500.0;
/// Where the two-stop gradient sits at the horizon: smoothstep(-0.2, 1, 0).
const TWO_STOP_HORIZON_T: f32 = 0.0741;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct SkyMaterial {
    #[uniform(0)]
    sky: SkyUniform,
}

#[derive(ShaderType, Debug, Clone)]
struct SkyUniform {
    top: LinearRgba,
    horizon: LinearRgba,
    bottom: LinearRgba,
    sun_dir: Vec4,
    sun_color: Vec4,
    has_horizon: u32,
    show_sun: u32,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://officexr_render/sky.wesl".into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Unit direction towards the sun and the disc's angular radius; a zero
/// offset points straight up.
pub fn sun_sky_params(position: Vec3, disc_radius: f32) -> (Vec3, f32) {
    let distance = position.length();
    if distance == 0.0 {
        return (Vec3::Y, disc_radius.atan());
    }
    (position / distance, (disc_radius / distance).atan())
}

impl SkyMaterial {
    pub fn new(background: &BackgroundConfig, lighting: &LightingConfig) -> Self {
        let top = background.top_color.to_linear();
        let bottom = background.bottom_color.to_linear();
        let horizon = background
            .horizon_color
            .map(|c| c.to_linear())
            .unwrap_or_else(|| bottom.mix(&top, TWO_STOP_HORIZON_T));
        let (dir, angular_radius) =
            sun_sky_params(Vec3::from(lighting.sun_position), lighting.sun_disc_radius);
        let sun = lighting.sun_color.to_linear();
        Self {
            sky: SkyUniform {
                top,
                horizon,
                bottom,
                sun_dir: dir.extend(angular_radius),
                sun_color: Vec4::new(sun.red, sun.green, sun.blue, lighting.sun_disc_intensity),
                has_horizon: background.horizon_color.is_some().into(),
                show_sun: lighting.show_sun_disc.into(),
            },
        }
    }
}

/// The dome as a child of the camera, so it always surrounds it.
pub(crate) fn sky_bundle(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<SkyMaterial>,
    material: SkyMaterial,
) -> impl Bundle {
    (
        Mesh3d(meshes.add(SphereMeshBuilder::new(SKY_RADIUS, default()).uv(64, 32))),
        MeshMaterial3d(materials.add(material)),
        NotShadowCaster,
        NotShadowReceiver,
        bevy::camera::visibility::NoFrustumCulling,
    )
}
