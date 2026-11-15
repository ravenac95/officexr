//! Touch input, as the web's `VirtualJoystick.tsx` and `useTouchCameraGestures.ts`: a fixed
//! bottom-left Bevy UI stick walks, one finger elsewhere looks and two pinch-zoom.

use std::collections::HashMap;

use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use officexr_render::views::View;
use serde::Deserialize;

use crate::look::{apply_zoom, is_look_mode};
use crate::{ControlsHub, window_of};

/// `useTouchCameraGestures.ts`'s drag-look radians per px.
pub const TOUCH_SENSITIVITY: f32 = 0.005;

const BASE_SIZE: f32 = 110.0;
const KNOB_SIZE: f32 = 46.0;
const MARGIN: f32 = 12.0;
const TRAVEL: f32 = (BASE_SIZE - KNOB_SIZE) / 2.0;
const DEAD_ZONE: f32 = 0.15;

/// Which on-screen stick feeds a view's [`crate::Controls::stick`]: the plugin's Bevy UI one,
/// or the host's (the web's DOM joystick, through `setStick`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JoystickKind {
    #[default]
    Bevy,
    Dom,
}

/// A finger down on a window, where it was last frame and now.
#[derive(Clone, Copy)]
struct Finger {
    id: u64,
    prev: Vec2,
    pos: Vec2,
}

/// The fingers down on each window, and the one holding the primary window's Bevy stick.
#[derive(Resource, Default)]
pub(crate) struct TouchState {
    fingers: HashMap<Entity, Vec<Finger>>,
    stick_touch: Option<u64>,
}

#[derive(Component)]
pub(crate) enum StickPart {
    Base,
    Knob,
}

/// As `joystick.ts`: an offset from the stick centre (screen px, y down) scaled by `radius`,
/// clamped to the unit circle and zero inside `dead_zone`, with y flipped to forward +.
pub fn joystick_vector(offset: Vec2, radius: f32, dead_zone: f32) -> Vec2 {
    let v = offset / radius;
    if v.length() < dead_zone {
        return Vec2::ZERO;
    }
    let v = v.clamp_length_max(1.0);
    Vec2::new(v.x, -v.y)
}

/// As `lookControls.ts`: the zoom factor for two fingers moving from `prev` to `next` apart.
pub fn pinch_factor(prev: f32, next: f32) -> f32 {
    if prev > 0.0 { next / prev } else { 1.0 }
}

/// The stick's centre in window px (y down), for a window of `size`.
fn stick_centre(size: Vec2) -> Vec2 {
    Vec2::new(MARGIN + BASE_SIZE / 2.0, size.y - MARGIN - BASE_SIZE / 2.0)
}

fn on_stick(at: Vec2, window: Vec2) -> bool {
    at.distance(stick_centre(window)) <= BASE_SIZE / 2.0
}

/// Hidden until the first touch, so the desktop never shows it.
pub(crate) fn spawn_stick(mut commands: Commands) {
    let circle = |size: f32, alpha: f32| {
        (
            Node {
                position_type: PositionType::Absolute,
                width: px(size),
                height: px(size),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, alpha)),
            Visibility::Hidden,
        )
    };
    commands.spawn((circle(BASE_SIZE, 0.15), StickPart::Base));
    commands.spawn((circle(KNOB_SIZE, 0.5), StickPart::Knob));
}

/// Applies each window's touches to the views on it with input enabled. Only the primary window
/// draws the Bevy stick.
// ponytail: views drawn to an image (webgl2 off the primary canvas) get no touch look or pinch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn read_touches(
    mut events: MessageReader<TouchInput>,
    primary: Option<Single<(Entity, &Window), With<PrimaryWindow>>>,
    views: Query<&View>,
    hub: Res<ControlsHub>,
    mut state: ResMut<TouchState>,
    mut parts: Query<(&StickPart, &mut Node, &mut Visibility)>,
) {
    let (primary, size) = primary.map_or((None, Vec2::ZERO), |p| (Some(p.0), p.1.size()));
    let mut hub = hub.lock();
    let views: Vec<_> = views
        .iter()
        .filter_map(|v| Some((v.id, window_of(v, primary)?)))
        .filter(|(id, _)| hub.get(id).is_some_and(|c| c.input_enabled))
        .collect();
    let bevy_stick = views
        .iter()
        .any(|(id, w)| Some(*w) == primary && hub[id].joystick == JoystickKind::Bevy);

    let state = &mut *state;
    for fingers in state.fingers.values_mut() {
        for f in fingers.iter_mut() {
            f.prev = f.pos;
        }
    }
    for event in events.read() {
        let fingers = state.fingers.entry(event.window).or_default();
        let at = event.position;
        match event.phase {
            TouchPhase::Started => {
                // A browser can drop a touch's end and reuse its id.
                fingers.retain(|f| f.id != event.id);
                fingers.push(Finger {
                    id: event.id,
                    prev: at,
                    pos: at,
                });
                if bevy_stick
                    && Some(event.window) == primary
                    && state.stick_touch.is_none()
                    && on_stick(at, size)
                {
                    state.stick_touch = Some(event.id);
                }
            }
            TouchPhase::Moved => {
                if let Some(f) = fingers.iter_mut().find(|f| f.id == event.id) {
                    f.pos = at;
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                fingers.retain(|f| f.id != event.id);
                if Some(event.window) == primary && state.stick_touch == Some(event.id) {
                    state.stick_touch = None;
                }
            }
        }
    }
    state.fingers.retain(|_, fingers| !fingers.is_empty());
    if !bevy_stick {
        state.stick_touch = None;
    }
    let no_fingers = Vec::new();
    let on_primary = primary
        .and_then(|w| state.fingers.get(&w))
        .unwrap_or(&no_fingers);
    let stick = state
        .stick_touch
        .and_then(|id| on_primary.iter().find(|f| f.id == id))
        .map_or(Vec2::ZERO, |f| {
            joystick_vector(f.pos - stick_centre(size), TRAVEL, DEAD_ZONE)
        });

    for (id, window) in &views {
        let Some(controls) = hub.get_mut(id) else {
            continue;
        };
        if Some(*window) == primary && controls.joystick == JoystickKind::Bevy {
            controls.stick = stick;
        }
        if !is_look_mode(controls.mode) {
            continue;
        }
        let gesture: Vec<_> = state
            .fingers
            .get(window)
            .unwrap_or(&no_fingers)
            .iter()
            .filter(|f| Some(f.id) != state.stick_touch)
            .collect();
        match gesture.as_slice() {
            [f] => controls.look_by(f.pos - f.prev, TOUCH_SENSITIVITY),
            [a, b] => apply_zoom(
                pinch_factor(a.prev.distance(b.prev), a.pos.distance(b.pos)),
                &mut controls.look.offsets,
            ),
            _ => {}
        }
    }

    // Stays shown once a touch device has revealed it.
    let show = bevy_stick && !on_primary.is_empty();
    let centre = stick_centre(size);
    let knob_at = centre + Vec2::new(stick.x, -stick.y) * TRAVEL;
    for (part, mut node, mut visibility) in &mut parts {
        match part {
            StickPart::Base => place(&mut node, centre, BASE_SIZE),
            StickPart::Knob => place(&mut node, knob_at, KNOB_SIZE),
        }
        if show {
            *visibility = Visibility::Visible;
        } else if !bevy_stick {
            *visibility = Visibility::Hidden;
        }
    }
}

fn place(node: &mut Node, centre: Vec2, size: f32) {
    node.left = px(centre.x - size / 2.0);
    node.top = px(centre.y - size / 2.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    // The expected values are `joystick.test.ts`'s and `lookControls.test.ts`'s.
    #[test]
    fn joystick_flips_y_to_forward_and_scales_by_radius() {
        assert_eq!(
            joystick_vector(Vec2::new(16.0, -16.0), 32.0, DEAD_ZONE),
            Vec2::new(0.5, 0.5)
        );
    }

    #[test]
    fn joystick_clamps_to_the_unit_circle() {
        let v = joystick_vector(Vec2::new(300.0, 400.0), 32.0, DEAD_ZONE);
        assert!((v.length() - 1.0).abs() < 1e-6);
        assert!((v - Vec2::new(0.6, -0.8)).length() < 1e-6);
    }

    #[test]
    fn joystick_ignores_the_dead_zone() {
        assert_eq!(
            joystick_vector(Vec2::new(3.0, 0.0), 32.0, DEAD_ZONE),
            Vec2::ZERO
        );
    }

    #[test]
    fn only_touches_on_the_stick_grab_it() {
        let window = Vec2::new(800.0, 600.0);
        assert!(on_stick(stick_centre(window), window));
        assert!(on_stick(Vec2::new(20.0, 540.0), window));
        assert!(!on_stick(Vec2::new(400.0, 300.0), window));
    }

    #[test]
    fn a_ten_px_drag_looks_a_twentieth_of_a_radian() {
        use crate::look::drag_look_delta;
        let d = drag_look_delta(Vec2::new(10.0, 0.0), TOUCH_SENSITIVITY, -1.0);
        assert!((d - Vec2::new(-0.05, 0.0)).length() < 1e-6);
        let inverted = drag_look_delta(Vec2::new(10.0, 0.0), TOUCH_SENSITIVITY, 1.0);
        assert!((inverted - Vec2::new(0.05, 0.0)).length() < 1e-6);
    }

    #[test]
    fn pinch_zooms_by_the_ratio_of_finger_distances() {
        assert_eq!(pinch_factor(100.0, 150.0), 1.5);
        assert_eq!(pinch_factor(100.0, 50.0), 0.5);
        assert_eq!(pinch_factor(0.0, 50.0), 1.0);
    }
}
