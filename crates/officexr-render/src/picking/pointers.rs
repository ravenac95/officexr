use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

use bevy::app::HierarchyPropagatePlugin;
use bevy::asset::uuid::Uuid;
use bevy::camera::NormalizedRenderTarget;
use bevy::ecs::event::PropagateEntityTrigger;
use bevy::picking::Pickable;
use bevy::picking::PickingSystems;
use bevy::picking::backend::ray::{RayId, RayMap};
use bevy::picking::backend::{HitData, PointerHits};
use bevy::picking::events::*;
use bevy::picking::pointer::{
    Location, PointerAction, PointerButton, PointerId, PointerInput, PointerMap,
};
use bevy::picking::prelude::MeshPickingPlugin;
use bevy::picking::{InteractionPlugin, PickingPlugin};
use bevy::prelude::*;
use serde::Serialize;

use super::{PickTarget, ray_at};
use crate::views::{View, ViewCamera, ViewId, find_view};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerMsgAction {
    Move,
    Press(PointerButton),
    Release(PointerButton),
    /// The pointer left the view, or the browser took it: it hovers nothing,
    /// and any press or drag ends without a release, until it moves back.
    Leave,
}

/// A pointer event on a view's canvas.
#[derive(Clone, Copy, Debug)]
pub struct PointerMsg {
    pub action: PointerMsgAction,
    /// Pixels from the view's top left.
    pub screen: Vec2,
    /// The view's size in the same pixels.
    pub size: Vec2,
    pub modifiers: Modifiers,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PickKind {
    Over,
    Out,
    Move,
    Down,
    Up,
    Click,
    DragStart,
    Drag,
    DragEnd,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PickEvent {
    pub kind: PickKind,
    pub target: PickTarget,
    /// Where the pointer hit, in world space; `drag` and `dragEnd` have none.
    pub point: Option<[f32; 3]>,
    pub normal: Option<[f32; 3]>,
    /// As DOM `MouseEvent.button`: 0 primary, 1 middle, 2 secondary.
    pub button: Option<u8>,
    pub modifiers: Modifiers,
    pub screen: [f32; 2],
}

/// A press on a target other than [`PickTarget::Ground`], written as Bevy's
/// picking runs in `PreUpdate`, so a host can keep the press from also
/// steering the camera.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct TargetPressed {
    pub view: ViewId,
    pub button: PointerButton,
}

#[derive(Default)]
struct Queues {
    /// Views whose presses only a ping bell announces: the play view, where any other takes pointer lock.
    bell_only: HashSet<ViewId>,
    pointer: Vec<(ViewId, PointerMsg)>,
    picks: Vec<(ViewId, PickEvent)>,
}

/// The views' pointer input and the pick events it raised, shareable with
/// callers outside the app. A view does no picking until its pointer's first message.
#[derive(Resource, Clone, Default)]
pub struct PickHub(Arc<Mutex<Queues>>);

impl PickHub {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn pointer(&self, view: ViewId, msg: PointerMsg) {
        self.lock().pointer.push((view, msg));
    }

    pub fn claim_only_bells(&self, view: ViewId, only: bool) {
        let mut hub = self.lock();
        if only {
            hub.bell_only.insert(view);
        } else {
            hub.bell_only.remove(&view);
        }
    }

    pub(super) fn push(&self, view: ViewId, pick: PickEvent) {
        self.lock().picks.push((view, pick));
    }

    pub fn take_picks(&self) -> Vec<(ViewId, PickEvent)> {
        std::mem::take(&mut self.lock().picks)
    }
}

/// A view's pointer, a child of the view alongside its [`PickTarget::Ground`].
#[derive(Component)]
pub(super) struct ViewPointer {
    pub(super) view: ViewId,
    ground: Entity,
    /// `None` while the pointer is off the view.
    pub(super) screen: Option<Vec2>,
    size: Vec2,
    pub(super) modifiers: Modifiers,
}

pub(crate) fn plugin(app: &mut App) {
    // `DefaultPlugins` brings these with Bevy's `bevy_picking` feature.
    if !app.is_plugin_added::<PickingPlugin>() {
        app.add_plugins((PickingPlugin, InteractionPlugin));
    }
    // An overlay made unpickable marks its meshes, however late they load.
    app.add_plugins((
        MeshPickingPlugin,
        HierarchyPropagatePlugin::<Pickable>::new(PostUpdate),
    ))
    .add_plugins(super::ping_bell::plugin)
    .init_resource::<PickHub>()
    .add_message::<TargetPressed>()
    .add_systems(First, feed_pointers.in_set(PickingSystems::Input))
    .add_systems(
        PreUpdate,
        (
            cast_rays
                .in_set(PickingSystems::ProcessInput)
                .after(RayMap::repopulate)
                .after(crate::views::drain_hub),
            hit_ground.in_set(PickingSystems::Backend),
        ),
    )
    .add_observer(forward::<PointerOver>)
    .add_observer(forward::<PointerOut>)
    .add_observer(forward::<PointerMove>)
    .add_observer(forward::<PointerPress>)
    .add_observer(forward::<PointerRelease>)
    .add_observer(forward::<PointerClick>)
    .add_observer(forward::<PointerDragStart>)
    .add_observer(forward::<PointerDrag>)
    .add_observer(forward::<PointerDragEnd>);
}

fn pointer_id(view: ViewId) -> PointerId {
    PointerId::Custom(Uuid::from_u128(view.into()))
}

/// Turns the hub's pointer messages into Bevy pointer input, spawning a
/// view's pointer and ground on its first.
fn feed_pointers(world: &mut World) {
    let mut inbox = std::mem::take(&mut world.resource::<PickHub>().lock().pointer).into_iter();
    while let Some((id, msg)) = inbox.next() {
        // Bevy clicks what was hovered the frame before a release, so a
        // release waits a frame after a press: a fast click arrives whole.
        if matches!(msg.action, PointerMsgAction::Press(_)) {
            let mut hub = world.resource::<PickHub>().lock();
            let later = std::mem::take(&mut hub.pointer);
            hub.pointer = inbox.by_ref().chain(later).collect();
        }
        let Some(view) = find_view(world, id) else {
            continue;
        };
        let pointer = pointer_id(id);
        let entity = match world.resource::<PointerMap>().get_entity(pointer) {
            Some(entity) => entity,
            None if msg.action == PointerMsgAction::Leave => continue,
            None => {
                let ground = world.spawn((PickTarget::Ground, ChildOf(view))).id();
                let state = ViewPointer {
                    view: id,
                    ground,
                    screen: None,
                    size: msg.size,
                    modifiers: msg.modifiers,
                };
                world.spawn((pointer, state, ChildOf(view))).id()
            }
        };
        let mut state = world.get_mut::<ViewPointer>(entity).unwrap();
        let last = state.screen;
        state.size = msg.size;
        state.modifiers = msg.modifiers;
        state.screen = (msg.action != PointerMsgAction::Leave).then_some(msg.screen);
        let action = match msg.action {
            PointerMsgAction::Move => PointerAction::Move {
                delta: last.map_or(Vec2::ZERO, |last| msg.screen - last),
            },
            PointerMsgAction::Press(button) => PointerAction::Press(button),
            PointerMsgAction::Release(button) => PointerAction::Release(button),
            // Bevy's own leave: it drops the pointer's hits this frame, so its
            // hover empties and `out` fires. The location stays, as `out` needs it.
            PointerMsgAction::Leave => PointerAction::Cancel,
        };
        let location = Location {
            // Matches no camera's target, so Bevy builds the pointer no ray of its own.
            target: NormalizedRenderTarget::None {
                width: msg.size.x as u32,
                height: msg.size.y as u32,
            },
            position: match msg.action {
                PointerMsgAction::Leave => last.unwrap_or(msg.screen),
                _ => msg.screen,
            },
        };
        world.write_message(PointerInput::new(pointer, location, action));
    }
}

/// Each pointer's ray through its view's camera, every frame, so hovering
/// follows a moving camera.
fn cast_rays(
    mut ray_map: ResMut<RayMap>,
    pointers: Query<(&PointerId, &ViewPointer, &ChildOf)>,
    views: Query<(&View, &ViewCamera)>,
) {
    for (&id, pointer, view) in &pointers {
        let Ok((view, camera)) = views.get(view.parent()) else {
            continue;
        };
        if view.paused {
            continue;
        }
        let ray = pointer.screen.and_then(|s| ray_at(camera, pointer.size, s));
        if let (Some(entity), Some(ray)) = (view.camera, ray) {
            ray_map.map.insert(RayId::new(entity, id), ray);
        }
    }
}

/// A picking backend for the floor plane, which has no mesh to ray-cast.
fn hit_ground(
    ray_map: Res<RayMap>,
    pointers: Query<(&PointerId, &ViewPointer, &ChildOf)>,
    views: Query<&View>,
    cameras: Query<&Camera>,
    mut hits: MessageWriter<PointerHits>,
) {
    for (&id, pointer, view) in &pointers {
        let Some(camera) = views.get(view.parent()).ok().and_then(|v| v.camera) else {
            continue;
        };
        let Some(ray) = ray_map.map.get(&RayId::new(camera, id)) else {
            continue;
        };
        let Some(depth) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) else {
            continue;
        };
        let hit = HitData::new(camera, depth, Some(ray.get_point(depth)), Some(Vec3::Y));
        let order = cameras.get(camera).map_or(0.0, |c| c.order as f32);
        hits.write(PointerHits::new(id, vec![(pointer.ground, hit)], order));
    }
}

/// The pointer events forwarded to the hub.
trait Forwarded: PointerEvent {
    const KIND: PickKind;
    fn button(&self) -> Option<PointerButton> {
        None
    }
    fn hit(&self) -> Option<&HitData> {
        None
    }
}

impl Forwarded for PointerOver {
    const KIND: PickKind = PickKind::Over;
    fn hit(&self) -> Option<&HitData> {
        Some(&self.hit)
    }
}

impl Forwarded for PointerOut {
    const KIND: PickKind = PickKind::Out;
    fn hit(&self) -> Option<&HitData> {
        Some(&self.hit)
    }
}

impl Forwarded for PointerMove {
    const KIND: PickKind = PickKind::Move;
    fn hit(&self) -> Option<&HitData> {
        Some(&self.hit)
    }
}

impl Forwarded for PointerPress {
    const KIND: PickKind = PickKind::Down;
    fn button(&self) -> Option<PointerButton> {
        Some(self.button)
    }
    fn hit(&self) -> Option<&HitData> {
        Some(&self.hit)
    }
}

impl Forwarded for PointerRelease {
    const KIND: PickKind = PickKind::Up;
    fn button(&self) -> Option<PointerButton> {
        Some(self.button)
    }
    fn hit(&self) -> Option<&HitData> {
        Some(&self.hit)
    }
}

impl Forwarded for PointerClick {
    const KIND: PickKind = PickKind::Click;
    fn button(&self) -> Option<PointerButton> {
        Some(self.button)
    }
    fn hit(&self) -> Option<&HitData> {
        Some(&self.hit)
    }
}

impl Forwarded for PointerDragStart {
    const KIND: PickKind = PickKind::DragStart;
    fn button(&self) -> Option<PointerButton> {
        Some(self.button)
    }
    fn hit(&self) -> Option<&HitData> {
        Some(&self.hit)
    }
}

impl Forwarded for PointerDrag {
    const KIND: PickKind = PickKind::Drag;
    fn button(&self) -> Option<PointerButton> {
        Some(self.button)
    }
}

impl Forwarded for PointerDragEnd {
    const KIND: PickKind = PickKind::DragEnd;
    fn button(&self) -> Option<PointerButton> {
        Some(self.button)
    }
}

/// Reports an event at the first [`PickTarget`] it bubbles up to, and stops
/// it there. Only views' pointers report, and they only get input while JS
/// listens, so `move` costs nothing for a view without an `onPick` callback.
// ponytail: a target made of several meshes (an avatar, a layout) reports `out` then `over` when the pointer crosses between them.
fn forward<E>(
    mut event: On<E>,
    targets: Query<&PickTarget>,
    pointers: Query<&ViewPointer>,
    pointer_map: Res<PointerMap>,
    hub: Res<PickHub>,
    mut pressed: MessageWriter<TargetPressed>,
) where
    E: Forwarded + for<'t> Event<Trigger<'t> = PropagateEntityTrigger<true, E, PointerTraversal>>,
{
    let pointer = event.pointer().clone();
    let Some(state) = pointer_map
        .get_entity(pointer.id)
        .and_then(|e| pointers.get(e).ok())
    else {
        return;
    };
    let Ok(target) = targets.get(event.event_target()) else {
        return;
    };
    event.propagate(false);
    if let (PickKind::Down, Some(button)) = (E::KIND, event.button())
        && *target != PickTarget::Ground
        && !hub.lock().bell_only.contains(&state.view)
    {
        pressed.write(TargetPressed {
            view: state.view,
            button,
        });
    }
    let hit = event.hit();
    let pick = PickEvent {
        kind: E::KIND,
        target: target.clone(),
        point: hit.and_then(|h| h.position).map(Into::into),
        normal: hit.and_then(|h| h.normal).map(Into::into),
        button: event.button().map(|b| match b {
            PointerButton::Primary => 0,
            PointerButton::Middle => 1,
            PointerButton::Secondary => 2,
        }),
        modifiers: state.modifiers,
        screen: pointer.position.into(),
    };
    hub.push(state.view, pick);
}

#[cfg(test)]
mod tests {
    use bevy::camera::CameraPlugin;

    use super::*;
    use crate::tests::{bare_app, scene};
    use crate::views::{ViewHub, ViewKind, ViewMsg, ViewTarget};
    use crate::{SceneCamera, WorldDiff};

    const SIZE: Vec2 = Vec2::new(64.0, 48.0);
    const CENTRE: Vec2 = Vec2::new(32.0, 24.0);
    const PRIMARY: PointerButton = PointerButton::Primary;

    fn app() -> App {
        let mut app = bare_app();
        app.add_plugins((TransformPlugin, CameraPlugin, plugin))
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();
        // Picks bubble up through a query that reads `Window`, which fails
        // while no window has registered it, as here.
        app.world_mut().register_component::<Window>();
        app
    }

    /// A view of the objects, a 2 m primitive cube each, seen from `from`.
    fn add_view(app: &mut App, objects: &[(&str, [f32; 3])], from: Vec3, at: Vec3) -> ViewId {
        let hub = app.world().resource::<ViewHub>().clone();
        let id = hub.create(ViewKind::World, |_| ViewTarget::Image(SIZE.as_uvec2()));
        let json = include_str!("../../examples/booper.scene.json");
        let mut diff: WorldDiff = serde_json::from_str(json).unwrap();
        diff.kinds = Some(vec![]);
        diff.objects = Some(scene(objects).objects);
        hub.send(id, ViewMsg::World(Box::new(diff)));
        hub.send(
            id,
            ViewMsg::Camera(SceneCamera {
                position: from.into(),
                look_at: at.into(),
                fov_deg: 60.0,
                near: 0.1,
                far: 100.0,
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        id
    }

    fn view_of(objects: &[(&str, [f32; 3])], from: Vec3, at: Vec3) -> (App, ViewId) {
        let mut app = app();
        let id = add_view(&mut app, objects, from, at);
        (app, id)
    }

    /// Object "a" at voxel 0 fills [0, 2]³; this view sees it at its centre.
    fn view_of_a() -> (App, ViewId) {
        view_of(&[("a", [0.0; 3])], Vec3::new(1.0, 6.0, 7.0), Vec3::ONE)
    }

    fn send(app: &App, id: ViewId, action: PointerMsgAction, screen: Vec2) {
        let msg = PointerMsg {
            action,
            screen,
            size: SIZE,
            modifiers: Modifiers::default(),
        };
        app.world().resource::<PickHub>().pointer(id, msg);
    }

    /// The view's pick events over the next two frames.
    fn picks(app: &mut App, id: ViewId) -> Vec<PickEvent> {
        app.update();
        app.update();
        let hub = app.world().resource::<PickHub>().clone();
        hub.take_picks()
            .into_iter()
            .filter(|(view, _)| *view == id)
            .map(|(_, pick)| pick)
            .collect()
    }

    fn kinds(picks: &[PickEvent]) -> Vec<PickKind> {
        picks.iter().map(|p| p.kind).collect()
    }

    fn click_centre(app: &mut App, id: ViewId) -> Vec<PickEvent> {
        send(app, id, PointerMsgAction::Move, CENTRE);
        send(app, id, PointerMsgAction::Press(PRIMARY), CENTRE);
        send(app, id, PointerMsgAction::Release(PRIMARY), CENTRE);
        let mut picks = picks(app, id);
        picks.retain(|p| p.kind == PickKind::Click);
        picks
    }

    fn a() -> PickTarget {
        PickTarget::Object { id: "a".into() }
    }

    #[test]
    fn clicking_an_object_picks_it_at_a_point_on_its_surface() {
        let (mut app, id) = view_of_a();
        let clicks = click_centre(&mut app, id);
        assert_eq!(clicks.len(), 1, "{clicks:?}");
        let click = &clicks[0];
        assert_eq!(click.target, a());
        assert_eq!(click.button, Some(0));
        let point = Vec3::from(click.point.unwrap());
        let from_faces = (point - Vec3::ONE).abs() - Vec3::ONE;
        assert!(
            from_faces.max_element() < 1e-3,
            "{point} is outside the cube"
        );
        assert!(
            from_faces.max_element() > -1e-3,
            "{point} is inside the cube"
        );
    }

    #[test]
    fn clicking_empty_floor_picks_the_ground() {
        let at = Vec3::new(10.0, 0.0, 10.0);
        let (mut app, id) = view_of(&[("a", [0.0; 3])], Vec3::new(10.0, 5.0, 15.0), at);
        let clicks = click_centre(&mut app, id);
        assert_eq!(clicks.len(), 1, "{clicks:?}");
        assert_eq!(clicks[0].target, PickTarget::Ground);
        let point = Vec3::from(clicks[0].point.unwrap());
        assert!(point.distance(at) < 1e-3, "{point}");
    }

    #[test]
    fn a_view_without_pointer_input_casts_no_rays() {
        let (mut app, _) = view_of_a();
        app.update();
        assert!(app.world().resource::<RayMap>().map.is_empty());
    }

    #[test]
    fn hovering_reports_over_and_moves_then_out_when_the_pointer_leaves() {
        let (mut app, id) = view_of_a();
        send(&app, id, PointerMsgAction::Move, CENTRE);
        let entered = picks(&mut app, id);
        let over = entered.iter().find(|p| p.kind == PickKind::Over);
        assert_eq!(over.map(|p| &p.target), Some(&a()), "{entered:?}");

        // Moving across the object reports each new hit, ground excluded.
        send(
            &app,
            id,
            PointerMsgAction::Move,
            CENTRE + Vec2::new(2.0, 1.0),
        );
        let moved = picks(&mut app, id);
        assert_eq!(kinds(&moved), [PickKind::Move], "{moved:?}");
        assert_eq!(moved[0].target, a());
        assert!(moved[0].point.is_some() && moved[0].normal.is_some());

        // No move follows the leave: `out` must come from the leave alone.
        send(&app, id, PointerMsgAction::Leave, Vec2::ZERO);
        let left = picks(&mut app, id);
        assert_eq!(kinds(&left), [PickKind::Out], "{left:?}");
        assert_eq!(left[0].target, a());
        assert!(picks(&mut app, id).is_empty(), "hover stuck after leaving");

        send(&app, id, PointerMsgAction::Move, CENTRE);
        assert!(kinds(&picks(&mut app, id)).contains(&PickKind::Over));
    }

    #[test]
    fn dragging_an_object_reports_start_drags_and_end_on_it() {
        let (mut app, id) = view_of_a();
        send(&app, id, PointerMsgAction::Move, CENTRE);
        send(&app, id, PointerMsgAction::Press(PRIMARY), CENTRE);
        let mut all = picks(&mut app, id);
        for dx in [3.0, 6.0] {
            send(
                &app,
                id,
                PointerMsgAction::Move,
                CENTRE + Vec2::new(dx, 0.0),
            );
            all.extend(picks(&mut app, id));
        }
        send(
            &app,
            id,
            PointerMsgAction::Release(PRIMARY),
            CENTRE + Vec2::new(6.0, 0.0),
        );
        all.extend(picks(&mut app, id));

        let drags: Vec<_> = all
            .iter()
            .filter(|p| {
                matches!(
                    p.kind,
                    PickKind::DragStart | PickKind::Drag | PickKind::DragEnd
                )
            })
            .collect();
        let drag_kinds: Vec<_> = drags.iter().map(|p| p.kind).collect();
        assert_eq!(
            drag_kinds,
            [
                PickKind::DragStart,
                PickKind::Drag,
                PickKind::Drag,
                PickKind::DragEnd
            ],
            "{all:?}"
        );
        assert!(drags.iter().all(|p| p.target == a() && p.button == Some(0)));
        assert!(drags[0].point.is_some(), "dragStart has the press's hit");
        assert!(drags[1..].iter().all(|p| p.point.is_none()));
        assert_eq!(drags[3].screen, (CENTRE + Vec2::new(6.0, 0.0)).to_array());
    }

    fn presses(app: &mut App) -> Vec<TargetPressed> {
        let mut messages = app.world_mut().resource_mut::<Messages<TargetPressed>>();
        messages.drain().collect()
    }

    #[test]
    fn a_press_on_a_target_but_not_the_ground_is_announced() {
        let (mut app, id) = view_of_a();
        click_centre(&mut app, id);
        let pressed = TargetPressed {
            view: id,
            button: PRIMARY,
        };
        assert_eq!(presses(&mut app), [pressed]);

        let at = Vec3::new(10.0, 0.0, 10.0);
        let (mut app, id) = view_of(&[("a", [0.0; 3])], Vec3::new(10.0, 5.0, 15.0), at);
        assert_eq!(click_centre(&mut app, id)[0].target, PickTarget::Ground);
        assert_eq!(presses(&mut app), []);
    }

    #[test]
    fn a_view_picks_only_its_own_scene() {
        let mut app = app();
        // A looks at where B's object is, but its own is far off.
        let a_view = add_view(
            &mut app,
            &[("a", [20.0, 0.0, 0.0])],
            Vec3::new(1.0, 6.0, 7.0),
            Vec3::ONE,
        );
        let b_view = add_view(
            &mut app,
            &[("b", [0.0; 3])],
            Vec3::new(1.0, 6.0, 7.0),
            Vec3::ONE,
        );
        let in_a = click_centre(&mut app, a_view);
        assert_eq!(in_a.len(), 1, "{in_a:?}");
        assert_eq!(in_a[0].target, PickTarget::Ground);
        let in_b = click_centre(&mut app, b_view);
        assert_eq!(in_b.len(), 1, "{in_b:?}");
        assert_eq!(in_b[0].target, PickTarget::Object { id: "b".into() });
    }

    fn set_overlays(app: &mut App, id: ViewId, overlays: serde_json::Value) {
        let overlays = serde_json::from_value(overlays).unwrap();
        let hub = app.world().resource::<ViewHub>().clone();
        hub.send(id, ViewMsg::Overlays(overlays));
        for _ in 0..3 {
            app.update();
        }
    }

    #[test]
    fn the_grid_lets_floor_clicks_through_to_the_ground_and_a_marker_reports_its_tag() {
        let at = Vec3::new(10.0, 0.0, 10.0);
        let (mut app, id) = view_of(&[("a", [0.0; 3])], Vec3::new(10.0, 5.0, 15.0), at);
        let grid = |pickable: Option<bool>| {
            serde_json::json!({"tag": "grid", "kind": "grid", "y": 0.01, "cellSize": 1,
                "fade": 30, "pickable": pickable})
        };
        set_overlays(&mut app, id, serde_json::json!([grid(None)]));
        assert_eq!(click_centre(&mut app, id)[0].target, PickTarget::Ground);
        assert_eq!(presses(&mut app), []);

        // Made pickable, the grid would take the click.
        set_overlays(&mut app, id, serde_json::json!([grid(Some(true))]));
        let overlay = |tag: &str| PickTarget::Overlay { tag: tag.into() };
        assert_eq!(click_centre(&mut app, id)[0].target, overlay("grid"));

        let spawn = serde_json::json!({"tag": "spawn", "kind": "marker", "shape": "spawnCone",
            "pos": at, "yaw": 0, "color": "#22d3ee"});
        set_overlays(&mut app, id, serde_json::json!([grid(None), spawn]));
        assert_eq!(click_centre(&mut app, id)[0].target, overlay("spawn"));
    }

    /// A named player whose avatar is a 2 m cube centred on the view, with its bell.
    fn view_with_bell() -> (App, ViewId, Entity) {
        let (mut app, id) = view_of(&[], Vec3::new(1.0, 6.0, 7.0), Vec3::ONE);
        let player = crate::PlayerRender {
            id: "pat".into(),
            name: "Pat".into(),
            ..default()
        };
        let players = crate::Players {
            players: vec![player],
            ..default()
        };
        app.world()
            .resource::<ViewHub>()
            .send(id, ViewMsg::Players(players));
        app.update();
        let view = find_view(app.world_mut(), id).unwrap();
        // The avatar systems need loaded rigs; a cube of its own stands in for the model.
        let cube = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::from_length(2.0));
        let avatar = app
            .world_mut()
            .spawn((
                PickTarget::Player { id: "pat".into() },
                Transform::from_xyz(1.0, 1.0, 1.0),
                Visibility::default(),
                Mesh3d(cube),
                ChildOf(view),
            ))
            .id();
        let bell = app
            .world_mut()
            .spawn((
                crate::avatars::PingBell::default(),
                crate::world_label::icon_badge(
                    Handle::default(),
                    28.0,
                    16.0,
                    Color::BLACK,
                    avatar,
                    Vec3::Y,
                ),
            ))
            .id();
        (app, id, bell)
    }

    /// Stands in for the UI's layout and placement: the bell is a 10 px square at the view's corner.
    fn lay_out_bell(app: &mut App, bell: Entity) {
        let mut entity = app.world_mut().entity_mut(bell);
        let mut node = entity.get_mut::<Node>().unwrap();
        node.left = Val::Px(0.0);
        node.top = Val::Px(0.0);
        entity.get_mut::<ComputedNode>().unwrap().size = Vec2::splat(10.0);
    }

    fn bell_shown(app: &App, bell: Entity) -> bool {
        app.world()
            .get::<crate::world_label::WorldLabel>(bell)
            .unwrap()
            .shown
    }

    #[test]
    fn hovering_a_named_player_shows_the_bell_and_it_outlasts_the_hover_while_the_pointer_is_on_it()
    {
        let (mut app, id, bell) = view_with_bell();
        lay_out_bell(&mut app, bell);
        app.update();
        assert!(!bell_shown(&app, bell), "no hover, no bell");

        send(&app, id, PointerMsgAction::Move, CENTRE);
        app.update();
        app.update();
        assert!(bell_shown(&app, bell));

        // Off the player and onto the bell: still shown, well past the linger.
        send(&app, id, PointerMsgAction::Move, Vec2::splat(5.0));
        for _ in 0..40 {
            app.update();
        }
        assert!(bell_shown(&app, bell));
    }

    #[test]
    fn a_press_and_release_on_the_bell_pings_once_and_claims_the_press() {
        let (mut app, id, bell) = view_with_bell();
        lay_out_bell(&mut app, bell);
        // The play view's configuration: only a bell's press is announced.
        app.world().resource::<PickHub>().claim_only_bells(id, true);
        send(&app, id, PointerMsgAction::Move, CENTRE);
        app.update();
        app.update();
        let on_bell = Vec2::splat(5.0);
        send(&app, id, PointerMsgAction::Move, on_bell);
        send(&app, id, PointerMsgAction::Press(PRIMARY), on_bell);
        send(&app, id, PointerMsgAction::Release(PRIMARY), on_bell);
        let clicks: Vec<_> = picks(&mut app, id)
            .into_iter()
            .filter(|p| {
                p.kind == PickKind::Click && matches!(p.target, PickTarget::PingBell { .. })
            })
            .collect();
        assert_eq!(clicks.len(), 1, "{clicks:?}");
        assert_eq!(clicks[0].target, PickTarget::PingBell { id: "pat".into() });
        let pressed = presses(&mut app);
        assert_eq!(pressed.len(), 1, "{pressed:?}");
    }

    #[test]
    fn releasing_off_the_bell_pings_nothing() {
        let (mut app, id, bell) = view_with_bell();
        lay_out_bell(&mut app, bell);
        send(&app, id, PointerMsgAction::Move, CENTRE);
        app.update();
        app.update();
        let on_bell = Vec2::splat(5.0);
        send(&app, id, PointerMsgAction::Move, on_bell);
        send(&app, id, PointerMsgAction::Press(PRIMARY), on_bell);
        send(
            &app,
            id,
            PointerMsgAction::Release(PRIMARY),
            Vec2::splat(30.0),
        );
        let picks = picks(&mut app, id);
        assert!(
            picks
                .iter()
                .all(|p| p.target != PickTarget::PingBell { id: "pat".into() })
        );
    }

    #[test]
    fn a_bell_only_view_announces_no_press_on_a_target() {
        let (mut app, id) = view_of_a();
        app.world().resource::<PickHub>().claim_only_bells(id, true);
        click_centre(&mut app, id);
        assert_eq!(presses(&mut app), []);
    }
}
