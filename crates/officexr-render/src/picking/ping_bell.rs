//! The hover ping bell: a [`PingBell`] label shows over a named player the
//! pointer hovers, and a click on it reports [`PickTarget::PingBell`].
//!
// OCP/SRP bend: the bell hit-tests its own rect and keeps its own press state
// instead of being a pick target like the rest. Bevy's UI picking backend
// needs a pointer whose `Location` targets a camera's render target, and the
// view pointers (`feed_pointers`) deliberately target none. Giving them a
// real target, and enabling `bevy/ui_picking`, would let `forward` report the
// bell and this module go.

use bevy::picking::PickingSystems;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput, PointerMap};
use bevy::prelude::*;

use super::pointers::ViewPointer;
use super::{PickEvent, PickHub, PickKind, PickTarget, TargetPressed};
use crate::avatars::PingBell;
use crate::views::{View, ViewKind, ViewPlayers};
use crate::world_label::{self, LabelOf, WorldLabel};

/// How long the bell outlasts the hover, for the pointer to travel up to it.
const LINGER_S: f32 = 0.4;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, show_bells)
        .add_systems(PreUpdate, click_bells.after(PickingSystems::Hover));
}

/// A bell is shown while its player, or the bell itself, is hovered, and a moment after.
#[allow(clippy::type_complexity)]
fn show_bells(
    time: Res<Time>,
    hover: Res<HoverMap>,
    pointers: Query<(&PointerId, &ViewPointer, &ChildOf)>,
    parents: Query<&ChildOf>,
    targets: Query<&PickTarget>,
    views: Query<(&View, &ViewPlayers)>,
    mut bells: Query<(
        &mut PingBell,
        &mut WorldLabel,
        &LabelOf,
        &Node,
        &ComputedNode,
    )>,
) {
    for (mut bell, mut label, anchor, node, computed) in &mut bells {
        let Some(view) = parents.get(anchor.0).ok().map(ChildOf::parent) else {
            continue;
        };
        let Ok(PickTarget::Player { id }) = targets.get(anchor.0) else {
            continue;
        };
        let Ok((kind, players)) = views.get(view) else {
            continue;
        };
        let named = players
            .players
            .iter()
            // An away claimant is pinged from their card instead.
            .any(|p| &p.id == id && !p.name.is_empty() && !p.frozen);
        let near = pointers.iter().any(|(pointer, state, v)| {
            v.parent() == view
                && (hover.get(pointer).is_some_and(|hits| {
                    hits.keys().any(|&hit| {
                        std::iter::once(hit)
                            .chain(parents.iter_ancestors(hit))
                            .any(|e| e == anchor.0)
                    })
                }) || label.shown
                    && state.screen.is_some_and(|s| {
                        world_label::rect(node, computed).is_some_and(|r| r.contains(s))
                    }))
        });
        if near {
            bell.linger = LINGER_S;
        } else {
            bell.linger = (bell.linger - time.delta_secs()).max(0.0);
        }
        let shown = kind.kind == ViewKind::World && !players.paused && named && bell.linger > 0.0;
        if label.shown != shown {
            label.shown = shown;
        }
    }
}

/// Press and release on a shown bell make a click, which the pointer's
/// view reports and the press keeps from also steering the camera.
#[allow(clippy::too_many_arguments)]
fn click_bells(
    mut inputs: MessageReader<PointerInput>,
    pointer_map: Res<PointerMap>,
    pointers: Query<(&ViewPointer, &ChildOf)>,
    parents: Query<&ChildOf>,
    targets: Query<&PickTarget>,
    mut bells: Query<(&mut PingBell, &WorldLabel, &LabelOf, &Node, &ComputedNode)>,
    hub: Res<PickHub>,
    mut pressed: MessageWriter<TargetPressed>,
) {
    for input in inputs.read() {
        let button = match input.action {
            PointerAction::Press(b) | PointerAction::Release(b) => b,
            _ => continue,
        };
        let Some((state, view)) = pointer_map
            .get_entity(input.pointer_id)
            .and_then(|e| pointers.get(e).ok())
        else {
            continue;
        };
        if button != PointerButton::Primary {
            continue;
        }
        for (mut bell, label, anchor, node, computed) in &mut bells {
            let on_bell = label.shown
                && parents
                    .get(anchor.0)
                    .is_ok_and(|p| p.parent() == view.parent())
                && world_label::rect(node, computed)
                    .is_some_and(|r| r.contains(input.location.position));
            match input.action {
                PointerAction::Press(_) if on_bell => {
                    bell.armed = true;
                    pressed.write(TargetPressed {
                        view: state.view,
                        button,
                    });
                    break;
                }
                PointerAction::Release(_) => {
                    let click = std::mem::take(&mut bell.armed) && on_bell;
                    if let (true, Ok(PickTarget::Player { id })) = (click, targets.get(anchor.0)) {
                        hub.push(
                            state.view,
                            PickEvent {
                                kind: PickKind::Click,
                                target: PickTarget::PingBell { id: id.clone() },
                                point: None,
                                normal: None,
                                button: Some(0),
                                modifiers: state.modifiers,
                                screen: input.location.position.into(),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }
}
