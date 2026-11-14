//! Views: each a camera, render target and scene of its own, kept apart by
//! `RenderLayers`, and driven through a [`ViewHub`] that any thread can feed.
//!
//! The [`WorldSceneData`], [`SceneCamera`] and [`Players`] resources, when
//! present, drive the primary view (id 0, layer 0) on the default render target.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use bevy::app::{HierarchyPropagatePlugin, Propagate, PropagateSet};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::gpu_readback::Readback;
use bevy::render::render_resource::{Extent3d, TextureFormat, TextureUsages};
use bevy::window::{PrimaryWindow, WindowOccluded, WindowRef};

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
    /// Bevy's primary window. Destroying the view leaves the window, idle.
    PrimaryWindow,
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

/// Mirroring the resources into the primary view, then every system that
/// turns views into entities. Order systems that change those resources
/// before it to draw the change the same frame.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SceneSystems;

#[derive(Component)]
pub struct View {
    pub id: ViewId,
    /// The lowest render layer free when the view was made, so layers stay few.
    layer: usize,
    pub(crate) kind: ViewKind,
    pub(crate) target: RenderTarget,
    pub(crate) camera: Option<Entity>,
    /// Hidden: its camera, once spawned, is inactive, and nothing is read back.
    pub(crate) paused: bool,
    settled: u32,
    /// The player ids its frame stats were last counted with.
    player_ids: Vec<String>,
}

impl View {
    pub fn target(&self) -> &RenderTarget {
        &self.target
    }
}

#[derive(Component, Deref)]
pub struct ViewScene(pub WorldSceneData);

#[derive(Component, Deref)]
pub struct ViewCamera(pub SceneCamera);

#[derive(Component, Deref, Default)]
pub struct ViewPlayers(pub Players);

/// The sun and aux light settings a view's lights and materials were last given.
#[derive(Component)]
pub(crate) struct Lit(pub crate::scene::LightingConfig);

fn view_bundle(id: ViewId, layer: usize, kind: ViewKind, target: RenderTarget) -> impl Bundle {
    (
        View {
            id,
            layer,
            kind,
            target,
            camera: None,
            paused: false,
            settled: 0,
            player_ids: vec![],
        },
        ObjectKinds::default(),
        ObjectIndex::default(),
        LayoutIndex::default(),
        ViewPlayers::default(),
        Propagate(RenderLayers::layer(layer)),
        Transform::default(),
        Visibility::default(),
    )
}

/// The view hub and the systems that spawn, feed and report on views, without
/// the renderer (`OfficeXrRenderPlugin` adds both).
pub fn plugin(app: &mut App) {
    app.init_resource::<ViewHub>()
        .add_message::<WindowOccluded>()
        .add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
        .configure_sets(
            PostUpdate,
            PropagateSet::<RenderLayers>::default().before(VisibilitySystems::CheckVisibility),
        )
        .add_systems(PreUpdate, (drain_hub, pause_occluded))
        .add_systems(Update, mirror_resources.in_set(SceneSystems))
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
            .spawn(view_bundle(0, 0, ViewKind::World, RenderTarget::default()))
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

fn free_layer(world: &mut World) -> usize {
    let used: HashSet<usize> = world
        .query::<&View>()
        .iter(world)
        .map(|v| v.layer)
        .collect();
    (1..).find(|l| !used.contains(l)).unwrap()
}

fn spawn_view(world: &mut World, id: ViewId, kind: ViewKind, target: ViewTarget) {
    let layer = free_layer(world);
    let view = world
        .spawn(view_bundle(id, layer, kind, RenderTarget::default()))
        .id();
    let target = match target {
        ViewTarget::PrimaryWindow => RenderTarget::default(),
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
            if size.x > 0 && size.y > 0 {
                world
                    .entity_mut(view)
                    .insert(Readback::texture(handle.clone()));
            } else {
                world.get_mut::<View>(view).unwrap().paused = true;
            }
            RenderTarget::Image(handle.into())
        }
    };
    world.get_mut::<View>(view).unwrap().target = target;
}

/// Resizes an image view, or pauses it, reading nothing back, at zero size.
fn resize(world: &mut World, entity: Entity, size: UVec2) {
    let Some(view) = world.get::<View>(entity) else {
        return;
    };
    let (RenderTarget::Image(ImageRenderTarget { handle, .. }), camera) =
        (&view.target, view.camera)
    else {
        return;
    };
    let handle = handle.clone();
    let active = size.x > 0 && size.y > 0;
    if active {
        if let Some(mut image) = world.resource_mut::<Assets<Image>>().get_mut(&handle) {
            image.resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
        }
        world.entity_mut(entity).insert(Readback::texture(handle));
    } else {
        world.entity_mut(entity).remove::<Readback>();
    }
    world.get_mut::<View>(entity).unwrap().paused = !active;
    if let Some(mut camera) = camera.and_then(|c| world.get_mut::<Camera>(c)) {
        camera.is_active = active;
    }
}

fn pause_occluded(
    mut occluded: MessageReader<WindowOccluded>,
    primary: Query<(), With<PrimaryWindow>>,
    mut views: Query<&mut View>,
    mut cameras: Query<&mut Camera>,
) {
    for event in occluded.read() {
        for mut view in &mut views {
            let on_window = match view.target {
                RenderTarget::Window(WindowRef::Entity(w)) => w == event.window,
                RenderTarget::Window(WindowRef::Primary) => primary.contains(event.window),
                _ => false,
            };
            if !on_window {
                continue;
            }
            view.paused = event.occluded;
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
        Ref<ViewPlayers>,
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
        // A player leaving changes no pending count, but does change what's drawn.
        let players_changed = players.is_changed()
            && !players
                .players
                .iter()
                .map(|p| &p.id)
                .eq(view.player_ids.iter());
        if players_changed {
            view.player_ids = players.players.iter().map(|p| p.id.clone()).collect();
        }
        let loaded = scene.is_some_and(|s| !s.is_changed()) && pending == 0 && !players_changed;
        view.settled = if loaded { view.settled + 1 } else { 0 };
        let Some(status) = hub.status.get_mut(&view.id) else {
            continue;
        };
        status.ready = view.settled >= SETTLE_FRAMES;
        status.pending = pending;
        // Meshes only come and go while a view loads, so its last count holds once it settles.
        if view.settled <= SETTLE_FRAMES {
            (status.draw_calls, status.triangles) = frame_stats(entity, &children, &drawn, &meshes);
        }
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
        view_on(app, ViewTarget::Window(canvas.into()))
    }

    fn view_on(app: &mut App, target: ViewTarget) -> ViewId {
        let hub = hub(app);
        let id = hub.create(ViewKind::World, |_| target);
        world_view_content(&hub, id);
        id
    }

    fn world_view_content(hub: &ViewHub, id: ViewId) {
        let json = include_str!("../examples/booper.scene.json");
        let mut diff: WorldDiff = serde_json::from_str(json).unwrap();
        diff.kinds = Some(vec![]);
        diff.objects = scene(&[("a", [0.0; 3]), ("b", [4.0, 0.0, 0.0])])
            .objects
            .into();
        hub.send(id, ViewMsg::World(Box::new(diff)));
        hub.send(id, ViewMsg::Camera(camera(0.0)));
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
            let layer = RenderLayers::layer(layer_of(&mut app, id));
            assert_eq!(layers.iter().filter(|l| **l == layer).count(), 2);
        }
    }

    fn layer_of(app: &mut App, id: ViewId) -> usize {
        let mut views = app.world_mut().query::<&View>();
        views.iter(app.world()).find(|v| v.id == id).unwrap().layer
    }

    #[test]
    fn a_destroyed_views_layer_is_reused() {
        let mut app = bare_app();
        let a = world_view(&mut app, "#a");
        let b = world_view(&mut app, "#b");
        app.update();
        let freed = layer_of(&mut app, a);
        hub(&app).send(a, ViewMsg::Destroy);
        app.update();

        let c = world_view(&mut app, "#c");
        app.update();
        assert_eq!(layer_of(&mut app, c), freed);
        assert_ne!(layer_of(&mut app, b), freed);
    }

    fn asset_counts(app: &App) -> [usize; 3] {
        let world = app.world();
        [
            world.resource::<Assets<Image>>().len(),
            world.resource::<Assets<ObjectMaterial>>().len(),
            world.resource::<Assets<Mesh>>().len(),
        ]
    }

    #[test]
    fn destroying_an_image_view_frees_its_assets() {
        let mut app = bare_app();
        // The first view leaves the shared primitive cube behind.
        let warm = view_on(&mut app, ViewTarget::Image(UVec2::new(8, 8)));
        app.update();
        hub(&app).send(warm, ViewMsg::Destroy);
        for _ in 0..3 {
            app.update();
        }
        let before = asset_counts(&app);

        let id = view_on(&mut app, ViewTarget::Image(UVec2::new(8, 8)));
        app.update();
        app.update();
        assert!(asset_counts(&app).iter().zip(before).all(|(n, b)| *n > b));

        hub(&app).send(id, ViewMsg::Destroy);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(asset_counts(&app), before);
    }

    #[test]
    fn pausing_an_image_view_stops_its_readback() {
        let mut app = bare_app();
        let id = view_on(&mut app, ViewTarget::Image(UVec2::new(8, 8)));
        app.update();
        assert!(has_readback(&mut app));
        hub(&app).send(id, ViewMsg::Resize(UVec2::ZERO));
        app.update();
        assert!(!has_readback(&mut app));
        hub(&app).send(id, ViewMsg::Resize(UVec2::new(4, 4)));
        app.update();
        assert!(has_readback(&mut app));
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

    fn camera_of(app: &mut App, id: ViewId) -> Entity {
        let mut views = app.world_mut().query::<&View>();
        views
            .iter(app.world())
            .find(|v| v.id == id)
            .unwrap()
            .camera
            .unwrap()
    }

    fn has_readback(app: &mut App) -> bool {
        let mut q = app
            .world_mut()
            .query_filtered::<(), (With<View>, With<Readback>)>();
        q.iter(app.world()).count() == 1
    }

    #[test]
    fn a_zero_sized_image_view_draws_and_reads_back_nothing() {
        let mut app = bare_app();
        let id = view_on(&mut app, ViewTarget::Image(UVec2::ZERO));
        app.update();
        assert!(!has_readback(&mut app));
        let camera = camera_of(&mut app, id);
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);

        hub(&app).send(id, ViewMsg::Resize(UVec2::new(4, 4)));
        app.update();
        assert!(has_readback(&mut app));
        assert!(app.world().get::<Camera>(camera).unwrap().is_active);
    }

    #[test]
    fn an_image_view_paused_before_its_camera_spawns_starts_inactive() {
        let mut app = bare_app();
        let hub = hub(&app);
        let id = hub.create(ViewKind::World, |_| ViewTarget::Image(UVec2::new(8, 8)));
        hub.send(id, ViewMsg::Resize(UVec2::ZERO));
        app.update();
        world_view_content(&hub, id);
        app.update();
        let camera = camera_of(&mut app, id);
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
        assert!(!has_readback(&mut app));
    }

    #[test]
    fn a_primary_window_view_draws_into_the_primary_window_and_leaves_it() {
        let mut app = bare_app();
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let id = view_on(&mut app, ViewTarget::PrimaryWindow);
        app.update();

        let camera = camera_of(&mut app, id);
        assert!(matches!(
            app.world().get::<RenderTarget>(camera),
            Some(RenderTarget::Window(WindowRef::Primary))
        ));
        assert!(layer_of(&mut app, id) >= 1);
        assert!(!has_readback(&mut app));

        hub(&app).send(id, ViewMsg::Destroy);
        app.update();
        assert!(app.world().get_entity(window).is_ok());
        assert!(app.world().get_entity(camera).is_err());
    }

    #[test]
    fn a_player_leaving_recounts_the_frame_stats() {
        let mut app = bare_app();
        let id = world_view(&mut app, "#c");
        let players = |ids: &[&str]| Players {
            players: ids
                .iter()
                .map(|id| crate::PlayerRender {
                    id: id.to_string(),
                    character: "Barbarian".into(),
                    pos: Vec3::ZERO,
                    yaw: 0.0,
                    vel: Vec3::ZERO,
                    name: String::new(),
                    airborne: false,
                })
                .collect(),
            ..default()
        };
        hub(&app).send(id, ViewMsg::Players(players(&["p"])));
        app.update();
        // Stands in for the player's avatar, which this app can't load.
        let view = find_view(app.world_mut(), id).unwrap();
        app.world_mut().spawn((Bound, ChildOf(view)));
        let settled = |app: &App| app.world().get::<View>(view).unwrap().settled;
        for _ in 0..SETTLE_FRAMES + 2 {
            app.update();
        }
        assert!(settled(&app) > SETTLE_FRAMES);

        hub(&app).send(id, ViewMsg::Players(players(&["p"])));
        app.update();
        assert!(settled(&app) > SETTLE_FRAMES, "the same players recounted");

        hub(&app).send(id, ViewMsg::Players(players(&[])));
        app.update();
        assert_eq!(settled(&app), 0);
    }
}
