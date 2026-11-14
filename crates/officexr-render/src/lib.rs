//! Bevy renderer for OfficeXR world objects, baked layouts, lighting,
//! shadows and sky, matching `packages/world/src/renderer/`.
//!
//! Add [`AssetBase`] before `DefaultPlugins` (asset sources must exist
//! before `AssetPlugin`), then [`OfficeXrRenderPlugin`], and insert a
//! [`WorldSceneData`] and a [`SceneCamera`]. Changing the first updates the
//! scene in place, by object id; changing the second only moves the camera.

mod avatars;
mod compressed_gltf;
mod layouts;
mod lighting;
#[cfg(feature = "mugshot")]
mod mugshot;
mod objects;
pub mod placement;
pub mod scene;
mod sky;

use bevy::asset::embedded_asset;
use bevy::asset::io::AssetSourceBuilder;
use bevy::camera::{Exposure, PerspectiveProjection, Projection};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::schedule::SystemCondition;
use bevy::ecs::schedule::common_conditions::{any_match_filter, resource_exists_and_changed};
use bevy::gltf::{
    DefaultGltfImageSampler, GltfLoader, GltfPlugin, extensions::GltfExtensionHandlers,
};
use bevy::image::CompressedImageFormatSupport;
use bevy::light::GlobalAmbientLight;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;

pub use avatars::{Bound as AvatarBound, Motion, PlayerRender, Players, next_motion};
pub use layouts::LayoutIndex;
pub use lighting::{HemisphereFill, ObjectMaterial, snap_to_shadow_texel};
#[cfg(feature = "mugshot")]
pub use mugshot::mugshot_player;
pub use objects::{ObjectIndex, ObjectKinds, apply_overrides};
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
        // Already absolute, e.g. a layout from the page's own origin.
        if gltf_path.starts_with("http://") || gltf_path.starts_with("https://") {
            return gltf_path.to_string();
        }
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
            scene_systems,
        ))
        .insert_resource(GlobalAmbientLight::NONE);
    }

    // Registered after `GltfPlugin::finish` so this loader takes over `.gltf`/`.glb`.
    // The duplicate-loader warning that logs is expected; the last loader registered
    // for an extension wins. Settings come from the app's `GltfPlugin`, like its own `finish`.
    fn finish(&self, app: &mut App) {
        let loader = {
            let fallback = GltfPlugin::default();
            let plugins = app.get_added_plugins::<GltfPlugin>();
            let plugin = plugins.first().copied().unwrap_or(&fallback);
            let world = app.world();
            GltfLoader {
                supported_compressed_formats: world
                    .get_resource::<CompressedImageFormatSupport>()
                    .map_or(default(), |support| support.0),
                custom_vertex_attributes: plugin.custom_vertex_attributes.clone(),
                default_sampler: world.resource::<DefaultGltfImageSampler>().get_internal(),
                default_convert_coordinates: plugin.convert_coordinates,
                extensions: world.resource::<GltfExtensionHandlers>().0.clone(),
                default_skinned_mesh_bounds_policy: plugin.skinned_mesh_bounds_policy,
                default_mesh_compression: plugin.mesh_compression.clone(),
            }
        };
        app.register_asset_loader(compressed_gltf::CompressedGltfLoader::new(loader));
    }
}

/// The systems that turn [`WorldSceneData`] and [`SceneCamera`] into entities.
fn scene_systems(app: &mut App) {
    app.init_resource::<objects::ObjectKinds>()
        .init_resource::<objects::ObjectIndex>()
        .init_resource::<layouts::LayoutIndex>()
        .add_systems(
            PostUpdate,
            apply_camera
                .run_if(resource_exists_and_changed::<SceneCamera>)
                .before(TransformSystems::Propagate),
        )
        .add_systems(
            Update,
            (
                (
                    objects::sync_kinds,
                    objects::register_primitives,
                    objects::sync_objects,
                    layouts::sync_layouts,
                )
                    .chain()
                    .run_if(scene_changed),
                (
                    objects::load_kinds,
                    objects::attach_models,
                    layouts::spawn_loaded_layouts,
                )
                    .chain()
                    .run_if(resource_exists::<WorldSceneData>),
                (
                    spawn_camera.run_if(resource_exists::<SceneCamera>),
                    sky::sync_background.run_if(
                        sky::background_changed.or_eager(any_match_filter::<Added<Camera3d>>),
                    ),
                )
                    .chain()
                    .run_if(resource_exists::<WorldSceneData>),
            )
                .chain(),
        );
}

fn scene_changed(scene: Option<Res<WorldSceneData>>) -> bool {
    scene.is_some_and(|s| s.is_changed())
}

fn camera_projection(camera: &SceneCamera) -> Projection {
    Projection::Perspective(PerspectiveProjection {
        fov: camera.fov_deg.to_radians(),
        near: camera.near,
        far: camera.far,
        ..default()
    })
}

fn camera_transform(camera: &SceneCamera) -> Transform {
    Transform::from_translation(camera.position.into()).looking_at(camera.look_at.into(), Vec3::Y)
}

fn spawn_camera(
    mut commands: Commands,
    camera: Res<SceneCamera>,
    existing: Query<(), With<Camera3d>>,
) {
    if !existing.is_empty() {
        return;
    }
    commands.spawn((
        Camera3d::default(),
        camera_projection(&camera),
        Tonemapping::AcesFitted,
        // An exposure multiplier of 1 / 0.6: three.js scales by that before its ACES fit.
        Exposure { ev100: -1.0 },
        camera_transform(&camera),
    ));
}

fn apply_camera(
    camera: Res<SceneCamera>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    for (mut transform, mut projection) in &mut cameras {
        *transform = camera_transform(&camera);
        *projection = camera_projection(&camera);
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use std::collections::HashMap;

    use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};

    use super::*;

    fn scene(objects: &[(&str, [f32; 3])]) -> WorldSceneData {
        let objects: Vec<_> = objects
            .iter()
            .map(|(id, position)| (*id, "__primitive_blue", *position))
            .collect();
        scene_of_kinds(&objects)
    }

    fn scene_of_kinds(objects: &[(&str, &str, [f32; 3])]) -> WorldSceneData {
        let mut json: serde_json::Value =
            serde_json::from_str(include_str!("../examples/booper.scene.json")).unwrap();
        json["kinds"] = serde_json::json!([]);
        json["objects"]["instances"] = objects
            .iter()
            .map(|(id, kind, position)| {
                serde_json::json!({"id": id, "kindId": kind, "position": position})
            })
            .collect();
        serde_json::from_value(json).unwrap()
    }

    fn camera(x: f32) -> SceneCamera {
        SceneCamera {
            position: [x, 5.0, 5.0],
            look_at: [0.0; 3],
            fov_deg: 60.0,
            near: 0.1,
            far: 100.0,
        }
    }

    fn bare_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<ObjectMaterial>()
            .init_asset::<SkyMaterial>()
            .init_asset::<Gltf>()
            .init_asset::<GltfNode>()
            .init_asset::<GltfMesh>()
            .init_asset::<GltfMaterial>()
            .insert_resource(AssetBase::Path(String::new()))
            .add_plugins(scene_systems);
        app
    }

    fn app(scene: WorldSceneData) -> App {
        let mut app = bare_app();
        app.insert_resource(scene).insert_resource(camera(0.0));
        app.update();
        app
    }

    fn live(app: &App) -> usize {
        app.world().iter_entities().count()
    }

    fn transforms(app: &mut App) -> HashMap<Entity, Transform> {
        let mut query = app
            .world_mut()
            .query_filtered::<(Entity, &Transform), With<objects::Placed>>();
        query.iter(app.world()).map(|(e, t)| (e, *t)).collect()
    }

    #[test]
    fn moving_an_object_changes_only_its_transform() {
        let mut app = app(scene(&[("a", [0.0; 3]), ("b", [4.0, 0.0, 0.0])]));
        let before = transforms(&mut app);
        assert_eq!(before.len(), 2);
        let b = app.world().resource::<ObjectIndex>().get("b").unwrap();

        *app.world_mut().resource_mut::<WorldSceneData>() =
            scene(&[("a", [0.0; 3]), ("b", [6.0, 0.0, 0.0])]);
        app.update();

        let after = transforms(&mut app);
        assert_eq!(after.len(), 2);
        for (entity, transform) in &before {
            assert_eq!(*transform != after[entity], *entity == b);
        }
    }

    #[test]
    fn adding_and_removing_an_object_spawns_and_despawns_one_entity() {
        let mut app = app(scene(&[("a", [0.0; 3])]));
        let a = app.world().resource::<ObjectIndex>().get("a").unwrap();
        let entities = live(&app);
        let before = transforms(&mut app);

        *app.world_mut().resource_mut::<WorldSceneData>() =
            scene(&[("a", [0.0; 3]), ("b", [4.0, 0.0, 0.0])]);
        app.update();
        assert_eq!(live(&app), entities + 1);
        assert_eq!(app.world().resource::<ObjectIndex>().get("a"), Some(a));

        *app.world_mut().resource_mut::<WorldSceneData>() = scene(&[("a", [0.0; 3])]);
        app.update();
        assert_eq!(app.world().resource::<ObjectIndex>().len(), 1);
        assert_eq!(app.world().resource::<ObjectIndex>().get("a"), Some(a));
        assert_eq!(live(&app), entities);
        assert_eq!(transforms(&mut app), before);
    }

    #[test]
    fn changing_an_objects_kind_keeps_its_entity_and_swaps_the_mesh() {
        let mut app = app(scene_of_kinds(&[("a", "__primitive_blue", [0.0; 3])]));
        let a = app.world().resource::<ObjectIndex>().get("a").unwrap();
        let mesh_of = |app: &App| {
            app.world()
                .get::<MeshMaterial3d<ObjectMaterial>>(a)
                .cloned()
        };
        let before = mesh_of(&app).unwrap();

        *app.world_mut().resource_mut::<WorldSceneData>() =
            scene_of_kinds(&[("a", "__primitive_stone", [0.0; 3])]);
        app.update();
        app.update();

        assert_eq!(app.world().resource::<ObjectIndex>().get("a"), Some(a));
        assert_ne!(mesh_of(&app).unwrap(), before);
    }

    #[test]
    fn changing_the_lighting_spawns_nothing() {
        let mut app = app(scene(&[("a", [0.0; 3])]));
        let entities = live(&app);

        app.world_mut()
            .resource_mut::<WorldSceneData>()
            .lighting
            .sun_intensity += 1.0;
        app.update();

        assert_eq!(live(&app), entities);
    }

    #[test]
    fn systems_wait_for_the_scene_and_the_camera() {
        let mut app = bare_app();
        app.update();
        app.insert_resource(scene(&[("a", [0.0; 3])]));
        app.update();
        app.insert_resource(camera(1.0));
        app.update();
        app.update();

        let mut cameras = app.world_mut().query_filtered::<(), With<Camera3d>>();
        assert_eq!(cameras.iter(app.world()).count(), 1);
    }

    #[test]
    fn moving_the_camera_spawns_and_despawns_nothing() {
        let mut app = app(scene(&[("a", [0.0; 3])]));
        let before = transforms(&mut app);
        let entities = live(&app);

        *app.world_mut().resource_mut::<SceneCamera>() = camera(3.0);
        app.update();

        assert_eq!(live(&app), entities);
        let mut cameras = app
            .world_mut()
            .query_filtered::<&Transform, With<Camera3d>>();
        assert_eq!(cameras.single(app.world()).unwrap().translation.x, 3.0);
        assert_eq!(transforms(&mut app), before);
    }
}
