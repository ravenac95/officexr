//! Touch movement, as the web's `VirtualJoystick.tsx` and `useTouchCameraGestures.ts`: a fixed
//! bottom-left stick walks, and dragging anywhere else looks.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::Orbit;

const BASE_SIZE: f32 = 110.0;
const KNOB_SIZE: f32 = 46.0;
const MARGIN: f32 = 12.0;
const TRAVEL: f32 = (BASE_SIZE - KNOB_SIZE) / 2.0;
const DEAD_ZONE: f32 = 0.15;

#[derive(Resource, Default)]
pub struct TouchControls {
    /// `x` strafe (right +), `y` forward (+), within the unit circle.
    pub stick: Vec2,
    stick_touch: Option<u64>,
    look_touch: Option<u64>,
}

#[derive(Component)]
pub enum StickPart {
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

/// The stick's centre in window px (y down), for a window of `size`.
pub fn stick_centre(size: Vec2) -> Vec2 {
    Vec2::new(MARGIN + BASE_SIZE / 2.0, size.y - MARGIN - BASE_SIZE / 2.0)
}

pub fn on_stick(at: Vec2, window: Vec2) -> bool {
    at.distance(stick_centre(window)) <= BASE_SIZE / 2.0
}

/// Hidden until the first touch, so the desktop never shows it.
pub fn spawn_stick(mut commands: Commands) {
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

pub fn read_touches(
    touches: Res<Touches>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut controls: ResMut<TouchControls>,
    mut orbit: ResMut<Orbit>,
    mut parts: Query<(&StickPart, &mut Node, &mut Visibility)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.size();
    for touch in touches.iter_just_pressed() {
        if controls.stick_touch.is_none() && on_stick(touch.position(), size) {
            controls.stick_touch = Some(touch.id());
        } else if controls.look_touch.is_none() {
            controls.look_touch = Some(touch.id());
        }
    }
    for touch in touches
        .iter_just_released()
        .chain(touches.iter_just_canceled())
    {
        if controls.stick_touch == Some(touch.id()) {
            controls.stick_touch = None;
        }
        if controls.look_touch == Some(touch.id()) {
            controls.look_touch = None;
        }
    }
    controls.stick = controls
        .stick_touch
        .and_then(|id| touches.get_pressed(id))
        .map_or(Vec2::ZERO, |t| {
            joystick_vector(t.position() - stick_centre(size), TRAVEL, DEAD_ZONE)
        });
    if let Some(t) = controls.look_touch.and_then(|id| touches.get_pressed(id)) {
        orbit.look(t.delta());
    }

    // Stays shown once a touch device has revealed it.
    let show = touches.iter().next().is_some();
    let centre = stick_centre(size);
    let knob_at = centre + Vec2::new(controls.stick.x, -controls.stick.y) * TRAVEL;
    for (part, mut node, mut visibility) in &mut parts {
        match part {
            StickPart::Base => place(&mut node, centre, BASE_SIZE),
            StickPart::Knob => place(&mut node, knob_at, KNOB_SIZE),
        }
        if show {
            *visibility = Visibility::Visible;
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
}
