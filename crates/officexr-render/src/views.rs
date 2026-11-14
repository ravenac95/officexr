//! Views: each a camera, render target and scene of its own, kept apart by
//! `RenderLayers`, and driven through a [`ViewHub`] that any thread can feed.
//!
//! The [`WorldSceneData`], [`SceneCamera`] and [`Players`] resources, when
//! present, drive the primary view (id 0) on the default render target.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use bevy::app::{HierarchyPropagatePlugin, Propagate, PropagateSet};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::gpu_readback::Readback;
use bevy::render::render_resource::{Extent3d, TextureFormat, TextureUsages};
use bevy::window::{WindowOccluded, WindowRef};

use crate::avatars::Bound;
use crate::lighting::ObjectMaterial;
use crate::{
    LayoutIndex, ObjectIndex, ObjectKinds, Players, SceneCamera, WorldDiff, WorldSceneData,
};

/// Frames drawn after every model has loaded before a view reports ready,
/// so swapped-in avatar materials have rendered.
const SETTLE_FRAMES: u32 = 3;

pub type ViewId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    World,
    /// A single character or kind model: no name tags.
    Preview,
}

pub enum ViewTarget {
    /// A window on the canvas this selector matches.
    Window(String),
    /// An image of this physical size, read back every frame; zero pauses the view.
    Image(UVec2),
}

pub enum ViewMsg {
    Create(ViewKind, ViewTarget),
    Destroy,
    World(Box<WorldDiff>),
    Players(Players),
    Camera(SceneCamera),
    /// For an image view: its new size, zero to pause it.
    Resize(UVec2),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewStatus {
    pub ready: bool,
    /// Models and avatars still loading.
    pub pending: usize,
    pub draw_calls: usize,
    pub triangles: usize,
}

#[derive(Default)]
struct Hub {
    next_id: ViewId,
    inbox: Vec<(ViewId, ViewMsg)>,
    status: HashMap<ViewId, ViewStatus>,
}

/// The app's view inbox and the views' last status, shareable with callers
/// outside the app.
#[derive(Resource, Clone, Default)]
pub struct ViewHub(Arc<Mutex<Hub>>);

impl ViewHub {
    fn lock(&self) -> MutexGuard<'_, Hub> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `target` gets the new view's id, e.g. to tag its canvas.
    pub fn create(&self, kind: ViewKind, target: impl FnOnce(ViewId) -> ViewTarget) -> ViewId {
        let mut hub = self.lock();
        hub.next_id += 1;
        let id = hub.next_id;
        hub.inbox.push((id, ViewMsg::Create(kind, target(id))));
        hub.status.insert(id, ViewStatus::default());
        id
    }

    /// False for a view that doesn't exist or was destroyed.
    pub fn send(&self, id: ViewId, msg: ViewMsg) -> bool {
        let mut hub = self.lock();
        let known = match msg {
            ViewMsg::Destroy => hub.status.remove(&id).is_some(),
            // Not ready until the new content has loaded, even before the app sees it.
            ViewMsg::World(_) => hub.status.get_mut(&id).map(|s| s.ready = false).is_some(),
            _ => hub.status.contains_key(&id),
        };
        if known {
            hub.inbox.push((id, msg));
        }
        known
    }

    pub fn status(&self, id: ViewId) -> Option<ViewStatus> {
        self.lock().status.get(&id).copied()
    }
}

#[derive(Component)]
pub struct View {
    pub id: ViewId,
    pub(crate) kind: ViewKind,
    pub(crate) target: RenderTarget,
    pub(crate) camera: Option<Entity>,
    settled: u32,
}

#[derive(Component, Deref, DerefMut)]
pub struct ViewScene(pub WorldSceneData);

#[derive(Component, Deref)]
pub struct ViewCamera(pub SceneCamera);

#[derive(Component, Deref, Default)]
pub struct ViewPlayers(pub Players);

/// The sun and aux light settings a view's lights and materials were last given.
#[derive(Component)]
pub(crate) struct Lit(pub crate::scene::LightingConfig);

fn view_bundle(id: ViewId, kind: ViewKind, target: RenderTarget) -> impl Bundle {
    (
        View {
            id,
            kind,
            target,
            camera: None,
            settled: 0,
        },
        ObjectKinds::default(),
        ObjectIndex::default(),
        LayoutIndex::default(),
        ViewPlayers::default(),
        Propagate(RenderLayers::layer(id as usize)),
        Transform::default(),
        Visibility::default(),
    )
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<ViewHub>()
        .add_message::<WindowOccluded>()
        .add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
        .configure_sets(
            PostUpdate,
            PropagateSet::<RenderLayers>::default().before(VisibilitySystems::CheckVisibility),
        )
        .add_systems(PreUpdate, (drain_hub, pause_occluded))
        // In `Update`, so resource changes made in `PreUpdate` draw this frame.
        .add_systems(Update, mirror_resources)
        .add_systems(Last, report_status);
}

pub(crate) fn mirror_resources(
    mut commands: Commands,
    scene: Option<Res<WorldSceneData>>,
    camera: Option<Res<SceneCamera>>,
    players: Option<Res<Players>>,
    views: Query<(Entity, &View)>,
) {
    if scene.is_none() && camera.is_none() && players.is_none() {
        return;
    }
    let primary = match views.iter().find(|(_, v)| v.id == 0) {
        Some((entity, _)) => entity,
        None => commands
            .spawn(view_bundle(0, ViewKind::World, RenderTarget::default()))
            .id(),
    };
    let mut primary = commands.entity(primary);
    if let Some(scene) = scene.filter(|s| s.is_changed()) {
        primary.insert(ViewScene(scene.clone()));
    }
    if let Some(camera) = camera.filter(|c| c.is_changed()) {
        primary.insert(ViewCamera(camera.clone()));
    }
    if let Some(players) = players.filter(|p| p.is_changed()) {
        primary.insert(ViewPlayers(players.clone()));
    }
}

fn find_view(world: &mut World, id: ViewId) -> Option<Entity> {
    world
        .query::<(Entity, &View)>()
        .iter(world)
        .find_map(|(entity, view)| (view.id == id).then_some(entity))
}

fn drain_hub(world: &mut World) {
    let inbox = std::mem::take(&mut world.resource::<ViewHub>().lock().inbox);
    for (id, msg) in inbox {
        let view = find_view(world, id);
        match (msg, view) {
            (ViewMsg::Create(kind, target), _) => spawn_view(world, id, kind, target),
            // Destroyed since.
            (_, None) => {}
            (ViewMsg::Destroy, Some(view)) => world.entity_mut(view).despawn(),
            (ViewMsg::World(diff), Some(view)) => {
                let mut entity = world.entity_mut(view);
                let scene = entity.take::<ViewScene>().map(|s| s.0);
                match diff.apply(scene) {
                    Some(scene) => {
                        entity.insert(ViewScene(scene));
                    }
                    None => {
                        warn!("view {id}: its first world update needs lighting and background")
                    }
                }
            }
            (ViewMsg::Players(players), Some(view)) => {
                world.entity_mut(view).insert(ViewPlayers(players));
            }
            (ViewMsg::Camera(camera), Some(view)) => {
                world.entity_mut(view).insert(ViewCamera(camera));
            }
            (ViewMsg::Resize(size), Some(view)) => resize(world, view, size),
        }
    }
}

fn spawn_view(world: &mut World, id: ViewId, kind: ViewKind, target: ViewTarget) {
    let view = world
        .spawn(view_bundle(id, kind, RenderTarget::default()))
        .id();
    let target = match target {
        ViewTarget::Window(canvas) => {
            let window = world
                .spawn((
                    Window {
                        canvas: Some(canvas),
                        fit_canvas_to_parent: true,
                        ..default()
                    },
                    ChildOf(view),
                ))
                .id();
            RenderTarget::Window(WindowRef::Entity(window))
        }
        ViewTarget::Image(size) => {
            let mut image = Image::new_target_texture(
                size.x.max(1),
                size.y.max(1),
                TextureFormat::Rgba8UnormSrgb,
                None,
            );
            image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            let handle = world.resource_mut::<Assets<Image>>().add(image);
            world
                .entity_mut(view)
                .insert(Readback::texture(handle.clone()));
            RenderTarget::Image(handle.into())
        }
    };
    world.get_mut::<View>(view).unwrap().target = target;
}

fn resize(world: &mut World, view: Entity, size: UVec2) {
    let Some(view) = world.get::<View>(view) else {
        return;
    };
    let (RenderTarget::Image(ImageRenderTarget { handle, .. }), camera) =
        (&view.target, view.camera)
    else {
        return;
    };
    let handle = handle.clone();
    if size.x > 0
        && size.y > 0
        && let Some(mut image) = world.resource_mut::<Assets<Image>>().get_mut(&handle)
    {
        image.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    // ponytail: a view paused before its camera spawns still draws once at its old size.
    if let Some(mut camera) = camera.and_then(|c| world.get_mut::<Camera>(c)) {
        camera.is_active = size.x > 0 && size.y > 0;
    }
}

fn pause_occluded(
    mut occluded: MessageReader<WindowOccluded>,
    views: Query<&View>,
    mut cameras: Query<&mut Camera>,
) {
    for event in occluded.read() {
        let on_window = |v: &&View| matches!(v.target, RenderTarget::Window(WindowRef::Entity(w)) if w == event.window);
        for view in views.iter().filter(on_window) {
            if let Some(mut camera) = view.camera.and_then(|c| cameras.get_mut(c).ok()) {
                camera.is_active = !event.occluded;
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn report_status(
    hub: Res<ViewHub>,
    mut views: Query<(
        Entity,
        &mut View,
        &ObjectKinds,
        &LayoutIndex,
        &ViewPlayers,
        Option<Ref<ViewScene>>,
    )>,
    bound: Query<&ChildOf, With<Bound>>,
    children: Query<&Children>,
    drawn: Query<(&Mesh3d, Option<&MeshMaterial3d<ObjectMaterial>>)>,
    meshes: Res<Assets<Mesh>>,
) {
    let mut hub = hub.lock();
    for (entity, mut view, kinds, layouts, players, scene) in &mut views {
        let bound = bound.iter().filter(|c| c.parent() == entity).count();
        let pending =
            kinds.pending() + layouts.pending() + players.players.len().saturating_sub(bound);
        let loaded = scene.is_some_and(|s| !s.is_changed()) && pending == 0;
        view.settled = if loaded { view.settled + 1 } else { 0 };
        let Some(status) = hub.status.get_mut(&view.id) else {
            continue;
        };
        let (draw_calls, triangles) = frame_stats(entity, &children, &drawn, &meshes);
        *status = ViewStatus {
            ready: view.settled >= SETTLE_FRAMES,
            pending,
            draw_calls,
            triangles,
        };
    }
}

/// Draws as one per mesh and object material, which Bevy batches into an
/// instanced draw, and the triangles of every mesh.
// ponytail: counted from the view's meshes, not measured on the GPU: no frustum culling or shadow passes.
fn frame_stats(
    view: Entity,
    children: &Query<&Children>,
    drawn: &Query<(&Mesh3d, Option<&MeshMaterial3d<ObjectMaterial>>)>,
    meshes: &Assets<Mesh>,
) -> (usize, usize) {
    let mut batches = HashSet::new();
    let mut triangles = 0;
    for (mesh, material) in drawn.iter_many(children.iter_descendants(view)).flatten() {
        batches.insert((mesh.id(), material.map(|m| m.id())));
        if let Some(mesh) = meshes.get(mesh) {
            triangles += mesh.indices().map_or(mesh.count_vertices(), |i| i.len()) / 3;
        }
    }
    (batches.len(), triangles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{bare_app, camera, scene};

    fn hub(app: &App) -> ViewHub {
        app.world().resource::<ViewHub>().clone()
    }

    fn world_view(app: &mut App, canvas: &str) -> ViewId {
        let hub = hub(app);
        let id = hub.create(ViewKind::World, |_| ViewTarget::Window(canvas.into()));
        let json = include_str!("../examples/booper.scene.json");
        let mut diff: WorldDiff = serde_json::from_str(json).unwrap();
        diff.kinds = Some(vec![]);
        diff.objects = scene(&[("a", [0.0; 3]), ("b", [4.0, 0.0, 0.0])])
            .objects
            .into();
        hub.send(id, ViewMsg::World(Box::new(diff)));
        hub.send(id, ViewMsg::Camera(camera(0.0)));
        id
    }

    fn live(app: &App) -> usize {
        app.world().iter_entities().count()
    }

    #[test]
    fn destroying_and_recreating_a_view_leaks_no_entities() {
        let mut app = bare_app();
        app.update();
        let before = live(&app);

        let first = world_view(&mut app, "#c");
        app.update();
        app.update();
        let populated = live(&app);
        assert!(populated > before + 2, "the view drew nothing");

        assert!(hub(&app).send(first, ViewMsg::Destroy));
        app.update();
        assert_eq!(live(&app), before);
        assert!(!hub(&app).send(first, ViewMsg::Players(Players::default())));

        world_view(&mut app, "#c");
        app.update();
        app.update();
        assert_eq!(live(&app), populated);
    }

    #[test]
    fn views_are_kept_apart_by_render_layers() {
        let mut app = bare_app();
        let a = world_view(&mut app, "#a");
        let b = world_view(&mut app, "#b");
        app.update();
        app.update();

        let mut placed = app
            .world_mut()
            .query_filtered::<&RenderLayers, With<crate::objects::Placed>>();
        let layers: Vec<_> = placed.iter(app.world()).cloned().collect();
        assert_eq!(layers.len(), 4);
        for id in [a, b] {
            let layer = RenderLayers::layer(id as usize);
            assert_eq!(layers.iter().filter(|l| **l == layer).count(), 2);
        }
    }

    #[test]
    fn is_ready_counts_pending_models_until_they_resolve() {
        let mut app = bare_app();
        let hub = hub(&app);
        let id = world_view(&mut app, "#c");
        let diff = serde_json::json!({
            "kinds": [{"id": "missing", "gltfPath": "/missing.gltf", "scale": 1, "tint": null,
                "opacity": 1, "roughness": null, "metalness": null, "emissive": null,
                "emissiveIntensity": 0}],
            "objects": {"instances": [{"id": "m", "kindId": "missing", "position": [0, 0, 0]}]},
        });
        hub.send(id, ViewMsg::World(serde_json::from_value(diff).unwrap()));
        app.update();

        let status = hub.status(id).unwrap();
        assert!(!status.ready);
        assert_eq!(status.pending, 1);

        for _ in 0..500 {
            app.update();
            if hub.status(id).unwrap().ready {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            hub.status(id).unwrap(),
            ViewStatus {
                ready: true,
                pending: 0,
                ..hub.status(id).unwrap()
            }
        );
    }

    #[test]
    fn frame_stats_count_a_populated_scene() {
        let mut app = bare_app();
        let id = world_view(&mut app, "#c");
        app.update();
        app.update();

        let status = hub(&app).status(id).unwrap();
        // The two cubes share one draw; the sky dome is the other.
        assert_eq!(status.draw_calls, 2);
        assert!(status.triangles > 2 * 12);
    }
}
