use std::sync::{Arc, Mutex, MutexGuard};

use bevy::asset::uuid::Uuid;
use bevy::camera::NormalizedRenderTarget;
use bevy::ecs::event::PropagateEntityTrigger;
use bevy::picking::PickingSystems;
use bevy::picking::backend::ray::{RayId, RayMap};
use bevy::picking::backend::{HitData, PointerHits};
use bevy::picking::events::*;
use bevy::picking::pointer::{
    Location, PointerAction, PointerButton, PointerId, PointerInput, PointerLocation, PointerMap,
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
    /// The pointer left the view: it hovers nothing until it moves back.
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
    /// Where the pointer hit, in world space; drags after their start have none.
    pub point: Option<[f32; 3]>,
    pub normal: Option<[f32; 3]>,
    /// As DOM `MouseEvent.button`: 0 primary, 1 middle, 2 secondary.
    pub button: Option<u8>,
    pub modifiers: Modifiers,
    pub screen: [f32; 2],
}

#[derive(Default)]
struct Queues {
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

    pub fn take_picks(&self) -> Vec<(ViewId, PickEvent)> {
        std::mem::take(&mut self.lock().picks)
    }
}

/// A view's pointer, a child of the view alongside its [`PickTarget::Ground`].
#[derive(Component)]
struct ViewPointer {
    view: ViewId,
    ground: Entity,
    /// `None` while the pointer is off the view.
    screen: Option<Vec2>,
    size: Vec2,
    modifiers: Modifiers,
}

pub(crate) fn plugin(app: &mut App) {
    // `DefaultPlugins` brings these with Bevy's `bevy_picking` feature.
    if !app.is_plugin_added::<PickingPlugin>() {
        app.add_plugins((PickingPlugin, InteractionPlugin));
    }
    app.add_plugins(MeshPickingPlugin)
        .init_resource::<PickHub>()
        .add_systems(First, feed_pointers.in_set(PickingSystems::Input))
        .add_systems(
            PreUpdate,
            (
                cast_rays
                    .in_set(PickingSystems::ProcessInput)
                    .after(RayMap::repopulate),
                hit_ground.in_set(PickingSystems::Backend),
            ),
        )
        .add_observer(forward::<PointerOver>)
        .add_observer(forward::<PointerOut>)
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
            PointerMsgAction::Leave => {
                world.get_mut::<PointerLocation>(entity).unwrap().location = None;
                continue;
            }
        };
        let location = Location {
            // Matches no camera's target, so Bevy builds the pointer no ray of its own.
            target: NormalizedRenderTarget::None {
                width: msg.size.x as u32,
                height: msg.size.y as u32,
            },
            position: msg.screen,
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

/// Reports an event at the first [`PickTarget`] it bubbles up to, and stops it there.
// ponytail: a target made of several meshes (an avatar, a layout) reports `out` then `over` when the pointer crosses between them.
fn forward<E>(
    mut event: On<E>,
    targets: Query<&PickTarget>,
    pointers: Query<&ViewPointer>,
    pointer_map: Res<PointerMap>,
    hub: Res<PickHub>,
) where
    E: Forwarded + for<'t> Event<Trigger<'t> = PropagateEntityTrigger<true, E, PointerTraversal>>,
{
    let Ok(target) = targets.get(event.event_target()) else {
        return;
    };
    event.propagate(false);
    let pointer = event.pointer();
    let Some(state) = pointer_map
        .get_entity(pointer.id)
        .and_then(|e| pointers.get(e).ok())
    else {
        return;
    };
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
    hub.lock().picks.push((state.view, pick));
}

#[cfg(test)]
mod tests {
    use bevy::camera::CameraPlugin;

    use super::*;
    use crate::tests::{bare_app, scene};
    use crate::views::{ViewHub, ViewKind, ViewMsg, ViewTarget};
    use crate::{SceneCamera, WorldDiff};

    const SIZE: Vec2 = Vec2::new(64.0, 48.0);

    /// A view of the objects, a 2 m primitive cube each, seen from `from`.
    fn view_of(objects: &[(&str, [f32; 3])], from: Vec3, at: Vec3) -> (App, ViewId) {
        let mut app = bare_app();
        app.add_plugins((TransformPlugin, CameraPlugin, plugin))
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();
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
        (app, id)
    }

    fn click_centre(app: &mut App, id: ViewId) -> Vec<PickEvent> {
        let hub = app.world().resource::<PickHub>().clone();
        for action in [
            PointerMsgAction::Move,
            PointerMsgAction::Press(PointerButton::Primary),
            PointerMsgAction::Release(PointerButton::Primary),
        ] {
            let modifiers = Modifiers::default();
            let (screen, size) = (SIZE / 2.0, SIZE);
            hub.pointer(
                id,
                PointerMsg {
                    action,
                    screen,
                    size,
                    modifiers,
                },
            );
        }
        app.update();
        app.update();
        hub.take_picks()
            .into_iter()
            .filter(|(view, pick)| *view == id && pick.kind == PickKind::Click)
            .map(|(_, pick)| pick)
            .collect()
    }

    #[test]
    fn clicking_an_object_picks_it_at_a_point_on_its_surface() {
        // Object "a" at voxel 0 fills [0, 2]³.
        let (mut app, id) = view_of(&[("a", [0.0; 3])], Vec3::new(1.0, 6.0, 7.0), Vec3::ONE);
        let clicks = click_centre(&mut app, id);
        assert_eq!(clicks.len(), 1, "{clicks:?}");
        let click = &clicks[0];
        assert_eq!(click.target, PickTarget::Object { id: "a".into() });
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
        let (mut app, _) = view_of(&[("a", [0.0; 3])], Vec3::new(1.0, 6.0, 7.0), Vec3::ONE);
        app.update();
        assert!(app.world().resource::<RayMap>().map.is_empty());
    }
}
