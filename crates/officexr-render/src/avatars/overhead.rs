//! Port of `characterHeight.ts` and `PlayerOverhead.tsx`'s anchor: each
//! model's bind-pose height, and where the stack over each head lands on
//! screen.
//!
//! The web page draws the stack (name and status tag, emote and hand
//! bubbles, an away claimant's card) as DOM over the canvas: status text
//! and emoji need the browser's fonts, and the card has buttons. So this
//! reports each head's anchor through [`ViewHub::anchors`] instead of
//! drawing it, as `setOverlays` is drawn for an editor's own DOM labels.

use bevy::prelude::*;

use super::{Avatar, PingBell};
use crate::views::{PlayerAnchor, View, ViewHub, ViewKind, ViewPlayers};
use crate::world_label::{LabelOf, Labels, WorldLabel};

/// `characterHeight.ts`'s `FALLBACK_CHARACTER_HEIGHT_M`: until measured.
pub const FALLBACK_HEIGHT_M: f32 = 2.3;
/// Between the top of the head and the bottom of the stack.
pub const OVERHEAD_GAP_M: f32 = 0.05;

/// How far a model reaches in `root`'s space: up from its origin, and out from its vertical axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ModelExtent {
    pub height: f32,
    pub radius: f32,
}

/// The bind-pose [`ModelExtent`] of the meshes under `root`, or `None`
/// before they have loaded.
pub(super) fn model_extent(
    root: Entity,
    children: &Query<&Children>,
    parents: &Query<&ChildOf>,
    transforms: &Query<&Transform>,
    mesh3d: &Query<&Mesh3d>,
    meshes: &Assets<Mesh>,
) -> Option<ModelExtent> {
    let mut top: Option<f32> = None;
    let mut radius = 0.0_f32;
    for e in children.iter_descendants(root) {
        let Some(mesh) = mesh3d.get(e).ok().and_then(|m| meshes.get(m)) else {
            continue;
        };
        let Some(positions) = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|p| p.as_float3())
        else {
            continue;
        };
        // The mesh's transform relative to `root`, from local transforms:
        // the global ones may not have propagated yet.
        let mut to_root = Mat4::IDENTITY;
        let mut at = e;
        while at != root {
            to_root = transforms
                .get(at)
                .map_or(Mat4::IDENTITY, Transform::to_matrix)
                * to_root;
            let Ok(parent) = parents.get(at) else {
                break;
            };
            at = parent.parent();
        }
        for p in positions {
            let p = to_root.transform_point3(Vec3::from(*p));
            top = Some(top.map_or(p.y, |t: f32| t.max(p.y)));
            radius = radius.max(p.xz().length());
        }
    }
    top.filter(|h| h.is_finite() && *h > 0.0)
        .map(|height| ModelExtent { height, radius })
}

/// The world point the stack over `avatar` stands on.
pub(super) fn anchor_point(feet: Vec3, height: f32) -> Vec3 {
    feet + Vec3::Y * (height + OVERHEAD_GAP_M)
}

/// Reports every view's avatars' anchors, once this frame's cameras and avatars have moved.
#[allow(clippy::type_complexity)]
pub(super) fn report_anchors(
    hub: Res<ViewHub>,
    views: Query<(Entity, &View, &ViewPlayers)>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    avatars: Query<(&Avatar, &GlobalTransform, &ChildOf, Option<&Labels>)>,
    bells: Query<&WorldLabel, (With<PingBell>, With<LabelOf>)>,
) {
    for (entity, view, players) in &views {
        let camera = view.camera.and_then(|c| cameras.get(c).ok());
        let anchors = avatars
            .iter()
            .filter(|(_, _, parent, _)| parent.parent() == entity)
            .map(|(avatar, transform, _, labels)| {
                let point = anchor_point(transform.translation(), avatar.height);
                let screen = camera.and_then(|(c, t)| c.world_to_viewport(t, point).ok());
                let bell = view.kind == ViewKind::World
                    && players.host_tags
                    && labels
                        .into_iter()
                        .flat_map(|l| l.iter())
                        .any(|l| bells.get(l).is_ok_and(|b| b.shown));
                PlayerAnchor {
                    id: avatar.id.clone(),
                    screen,
                    height: avatar.height,
                    bell,
                }
            })
            .collect();
        hub.set_anchors(view.id, anchors);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    #[test]
    fn the_stack_stands_just_over_the_head() {
        let at = anchor_point(Vec3::new(1.0, 0.5, 2.0), 2.0);
        assert!(at.distance(Vec3::new(1.0, 2.55, 2.0)) < 1e-6);
    }

    #[test]
    fn a_models_extent_is_its_highest_and_widest_vertex_through_its_transforms() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1.0, 2.0, 1.0));
        let root = app.world_mut().spawn(Transform::default()).id();
        let armature = app
            .world_mut()
            .spawn((Transform::from_xyz(0.0, 0.5, 0.0), ChildOf(root)))
            .id();
        app.world_mut().spawn((
            Mesh3d(mesh),
            Transform::from_xyz(0.0, 1.0, 0.0),
            ChildOf(armature),
        ));
        let extent = app
            .world_mut()
            .run_system_once(
                move |children: Query<&Children>,
                      parents: Query<&ChildOf>,
                      transforms: Query<&Transform>,
                      mesh3d: Query<&Mesh3d>,
                      meshes: Res<Assets<Mesh>>| {
                    model_extent(root, &children, &parents, &transforms, &mesh3d, &meshes)
                },
            )
            .unwrap();
        let extent = extent.unwrap();
        assert_eq!(extent.height, 2.5);
        assert!((extent.radius - 0.5_f32.hypot(0.5)).abs() < 1e-6);
    }
}
