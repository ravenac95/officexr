//! Bevy renderer for OfficeXR world objects, baked layouts, lighting,
//! shadows and sky, matching `packages/world/src/renderer/`.
//!
//! Add [`AssetBase`] before `DefaultPlugins` (asset sources must exist
//! before `AssetPlugin`), then [`OfficeXrRenderPlugin`], and insert a
//! [`WorldSceneData`] and a [`SceneCamera`]. Changing either respawns the
//! scene.

mod avatars;
mod layouts;
mod lighting;
mod objects;
pub mod placement;
pub mod scene;
mod sky;

use bevy::asset::embedded_asset;
use bevy::asset::io::AssetSourceBuilder;
use bevy::camera::{Exposure, PerspectiveProjection, Projection};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::GlobalAmbientLight;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;

pub use avatars::{Motion, PlayerRender, Players, next_motion};
pub use layouts::PendingLayouts;
pub use lighting::{HemisphereFill, ObjectMaterial, snap_to_shadow_texel};
pub use objects::{PendingKinds, apply_overrides};
pub use scene::*;
pub use sky::{SkyMaterial, sun_sky_params};

const CAMERA_FAR: f32 = 2000.0;
const LOCAL_SOURCE: &str = "officexr";

/// Where kind `gltfPath`s (e.g. `/models/blocks/x.gltf`) resolve: a local
/// directory, or an `http(s)://` origin.
#[derive(Resource, Clone, Debug)]
pub enum AssetBase {
    Path(String),
    Http(String),
}

impl AssetBase {
    /// An `http(s)://` origin, else a directory.
    pub fn parse(base: String) -> Self {
        if base.starts_with("http://") || base.starts_with("https://") {
            AssetBase::Http(base)
        } else {
            AssetBase::Path(base)
        }
    }

    pub fn asset_path(&self, gltf_path: &str) -> String {
        let rel = gltf_path.trim_start_matches('/');
        match self {
            AssetBase::Path(_) => format!("{LOCAL_SOURCE}://{rel}"),
            AssetBase::Http(origin) => format!("{}/{rel}", origin.trim_end_matches('/')),
        }
    }
}

impl Plugin for AssetBase {
    fn build(&self, app: &mut App) {
        if let AssetBase::Path(dir) = self {
            app.register_asset_source(
                LOCAL_SOURCE,
                AssetSourceBuilder::platform_default(dir, None),
            );
        }
        app.insert_resource(self.clone());
    }
}

/// The viewer camera, as three.js's perspective camera.
#[derive(Resource, Clone, Debug)]
pub struct SceneCamera {
    pub position: [f32; 3],
    pub look_at: [f32; 3],
    pub fov_deg: f32,
    pub near: f32,
    pub far: f32,
}

/// Marks everything the plugin spawned for the current scene.
#[derive(Component)]
struct SceneEntity;

pub struct OfficeXrRenderPlugin;

impl Plugin for OfficeXrRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "hemisphere.wesl");
        embedded_asset!(app, "sky.wesl");
        app.add_plugins((
            MaterialPlugin::<ObjectMaterial>::default(),
            MaterialPlugin::<SkyMaterial>::default(),
            lighting::plugin,
            avatars::plugin,
        ))
        .insert_resource(GlobalAmbientLight::NONE)
        .init_resource::<objects::PendingKinds>()
        .init_resource::<layouts::PendingLayouts>()
        .add_systems(
            Update,
            (
                (
                    despawn_scene,
                    spawn_camera,
                    lighting::spawn_lights,
                    objects::request_kinds,
                    objects::spawn_primitives,
                    layouts::request_layouts,
                )
                    .chain()
                    .run_if(scene_changed),
                (objects::spawn_loaded_kinds, layouts::spawn_loaded_layouts)
                    .run_if(resource_exists::<WorldSceneData>),
            )
                .chain(),
        );
    }
}

fn scene_changed(scene: Option<Res<WorldSceneData>>, camera: Option<Res<SceneCamera>>) -> bool {
    match (scene, camera) {
        (Some(scene), Some(camera)) => scene.is_changed() || camera.is_changed(),
        _ => false,
    }
}

fn despawn_scene(mut commands: Commands, entities: Query<Entity, With<SceneEntity>>) {
    for e in &entities {
        commands.entity(e).despawn();
    }
}

fn spawn_camera(
    mut commands: Commands,
    camera: Res<SceneCamera>,
    scene: Res<WorldSceneData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut skies: ResMut<Assets<SkyMaterial>>,
) {
    let mut entity = commands.spawn((
        SceneEntity,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: camera.fov_deg.to_radians(),
            near: camera.near,
            far: camera.far,
            ..default()
        }),
        Tonemapping::AcesFitted,
        // An exposure multiplier of 1 / 0.6: three.js scales by that before its ACES fit.
        Exposure { ev100: -1.0 },
        Transform::from_translation(camera.position.into())
            .looking_at(camera.look_at.into(), Vec3::Y),
    ));
    match scene.clear_color {
        // The clear colour skips tonemapping, as three.js's `scene.background` does.
        Some(color) => {
            entity.insert(Camera {
                clear_color: ClearColorConfig::Custom(color),
                ..default()
            });
        }
        None => {
            let sky = SkyMaterial::new(&scene.background, &scene.lighting);
            entity.with_child(sky::sky_bundle(&mut meshes, &mut skies, sky));
        }
    }
}
