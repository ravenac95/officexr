//! Port of `characterSilhouette.ts`: an avatar hidden behind a wall shows
//! through it as a flat, translucent shape in its player's colour.
//!
//! Each of the character's meshes gets a twin sharing its mesh and skin,
//! drawn with [`SilhouetteMaterial`], which passes only where the depth test
//! fails. three.js stencils the body out so limbs crossing the torso aren't
//! tinted; Bevy's material API has no stencil, so the test is instead made
//! from a point [`REACH_M`] nearer the camera, which the body's own depth
//! never hides.

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
/// Deeper than any limb crosses in front of the torso, shallower than the
/// thinnest wall gap a player stands behind.
const REACH_M: f32 = 0.45;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct SilhouetteMaterial {
    #[uniform(0)]
    uniform: SilhouetteUniform,
}

#[derive(ShaderType, Debug, Clone)]
struct SilhouetteUniform {
    color: LinearRgba,
    reach: f32,
}

impl SilhouetteMaterial {
    pub fn new(color: Color) -> Self {
        Self {
            uniform: SilhouetteUniform {
                color: color.to_linear().with_alpha(SILHOUETTE_OPACITY),
                reach: REACH_M,
            },
        }
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
