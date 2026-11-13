use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use officexr_render::SceneCamera;

const ROTATE_PER_PX: f32 = 0.005;
const PAN_PER_PX: f32 = 0.002;
const ZOOM_PER_LINE: f32 = 0.1;

/// Left-drag (or a touch drag, see [`crate::touch`]) orbits, right-drag pans, the wheel zooms. Once the local player spawns it
/// follows them: [`crate::player`] moves `target` every frame.
#[derive(Resource)]
pub struct Orbit {
    pub target: Vec3,
    /// Also the heading WASD moves relative to.
    pub yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: -0.5,
            distance: 8.0,
        }
    }
}

impl Orbit {
    /// Turns the view by a drag of `delta` px, as mouse and touch look do on the web.
    pub fn look(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ROTATE_PER_PX;
        self.pitch = (self.pitch - delta.y * ROTATE_PER_PX).clamp(-1.55, -0.05);
    }

    fn transform(&self) -> Transform {
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        Transform::from_translation(self.target + rotation * Vec3::Z * self.distance)
            .looking_at(self.target, Vec3::Y)
    }
}

/// The plugin's camera; inserted once, since changing it respawns the scene.
pub fn initial() -> SceneCamera {
    let t = Orbit::default().transform().translation;
    SceneCamera {
        position: t.into(),
        look_at: [0.0; 3],
        fov_deg: 60.0,
        near: 0.1,
        far: 2000.0,
    }
}

pub fn orbit_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut orbit: ResMut<Orbit>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    let delta = motion.delta;
    if buttons.pressed(MouseButton::Left) {
        orbit.look(delta);
    }
    if buttons.pressed(MouseButton::Right) {
        let (yaw, distance) = (Quat::from_rotation_y(orbit.yaw), orbit.distance);
        orbit.target += yaw * Vec3::new(-delta.x, 0.0, -delta.y) * PAN_PER_PX * distance;
    }
    orbit.distance = (orbit.distance * (1.0 - scroll.delta.y * ZOOM_PER_LINE)).clamp(2.0, 500.0);
    for mut transform in &mut cameras {
        *transform = orbit.transform();
    }
}
