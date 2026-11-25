//! Port of `characterSilhouette.ts`: an avatar hidden behind a wall shows
//! through it as a flat, translucent shape in its player's colour.
//!
//! Each of the character's meshes gets a twin sharing its mesh and skin,
//! drawn with [`SilhouetteMaterial`], which passes only where the depth test
//! fails. three.js stencils the body out so its own head and limbs crossing
//! it aren't tinted; Bevy's depth target has no stencil, so the test is
//! instead made from where the view ray enters a vertical capsule around the
//! avatar ([`SilhouetteMaterial::set_capsule`]): the body's own parts all lie
//! inside it, a wall in front lies outside.

use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

/// `silhouetteColor.ts`'s `SILHOUETTE_OPACITY`.
pub const SILHOUETTE_OPACITY: f32 = 0.5;
/// Room round the bind-pose model for poses, swings and bumps.
const CAPSULE_SLACK_M: f32 = 0.15;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct SilhouetteMaterial {
    #[uniform(0)]
    uniform: SilhouetteUniform,
}

#[derive(ShaderType, Debug, Clone)]
struct SilhouetteUniform {
    color: LinearRgba,
    /// The capsule's axis, feet to head, in world space.
    base: Vec3,
    radius: f32,
    top: Vec3,
}

impl SilhouetteMaterial {
    pub fn new(color: Color) -> Self {
        Self {
            uniform: SilhouetteUniform {
                color: color.to_linear().with_alpha(SILHOUETTE_OPACITY),
                base: Vec3::ZERO,
                radius: 0.0,
                top: Vec3::ZERO,
            },
        }
    }

    /// Wraps the avatar in `capsule`.
    pub fn set_capsule(&mut self, capsule: Capsule) {
        let u = &mut self.uniform;
        (u.base, u.top, u.radius) = (capsule.base, capsule.top, capsule.radius);
    }

    /// Whether `capsule` is far enough from the one set to be worth re-uploading.
    pub fn capsule_moved(&self, capsule: &Capsule) -> bool {
        let u = &self.uniform;
        u.base.distance(capsule.base) >= 0.01
            || u.top.distance(capsule.top) >= 0.01
            || (u.radius - capsule.radius).abs() >= 0.01
    }
}

impl Material for SilhouetteMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://officexr_render/avatars/silhouette.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_shadows() -> bool {
        false
    }

    fn enable_prepass() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = &mut descriptor.depth_stencil {
            // Reverse Z: a fragment farther than what's drawn passes.
            depth.depth_compare = Some(CompareFunction::Less);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// A vertical capsule round an avatar, in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    base: Vec3,
    top: Vec3,
    radius: f32,
}

impl Capsule {
    /// Round a model standing at `feet`, `height` tall and reaching `reach`
    /// out from its axis, with room to move.
    pub fn new(feet: Vec3, height: f32, reach: f32) -> Self {
        Self {
            base: feet,
            top: feet + Vec3::Y * height,
            radius: reach + CAPSULE_SLACK_M,
        }
    }
}

/// A twin drawing its parent mesh's silhouette.
#[derive(Component)]
pub(super) struct SilhouetteTwin;

/// A hidden twin of the mesh `source`, posed by the same skin.
pub(super) fn twin(
    commands: &mut Commands,
    source: Entity,
    mesh: &Mesh3d,
    skin: Option<&SkinnedMesh>,
    material: Handle<SilhouetteMaterial>,
) {
    let mut twin = commands.spawn((
        SilhouetteTwin,
        mesh.clone(),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
        bevy::camera::visibility::NoFrustumCulling,
        ChildOf(source),
    ));
    if let Some(skin) = skin {
        twin.insert(skin.clone());
    }
    #[cfg(feature = "picking")]
    twin.insert(bevy::picking::Pickable::IGNORE);
}
