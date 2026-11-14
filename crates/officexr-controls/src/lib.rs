//! Keyboard and mouse input and the gameplay camera modes, per view.
//!
//! Each view with controls has a [`Controls`] entry in the [`ControlsHub`],
//! which callers outside the app (the web API, the native client) feed and
//! read. Every frame the plugin turns held keys into a [`MoveIntent`] relative
//! to the camera's yaw, and the mouse and the followed player into the view's
//! [`ViewCamera`]. The caller steps the sim with the intent and hands the
//! player's new position back with [`Controls::set_follow`].
//!
//! Keys are read from Bevy's `ButtonInput<KeyCode>`, which on the web hears
//! the whole page, so they reach any view whose input is enabled. Pointer lock,
//! mouse-look and the wheel need the view to draw into a Bevy window; on
//! WebGL2 only the primary view does.
// ponytail: a WebGL2 image view (non-primary canvas) gets keys but no pointer lock, mouse-look or wheel; feed it from JS if one ever needs them.

mod config;
pub mod look;
mod rig;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use bevy::camera::RenderTarget;
use bevy::input::ButtonState;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowRef};
use officexr_render::views::{SceneSystems, View, ViewCamera, ViewId};
use serde::{Deserialize, Serialize};

pub use config::{CameraConfig, FixedCamera};
use look::{
    Look, MOUSE_SENSITIVITY, apply_look_delta, apply_zoom, dom_wheel_delta, fixed_movement_yaw,
    is_look_mode, is_yaw_pitch_mode, wheel_zoom_factor,
};
use rig::{FlyKeys, Rig};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraMode {
    FirstPerson,
    ThirdPerson,
    /// Parked at an azimuth and pitch, leashed to the player.
    #[default]
    Fixed,
    /// Straight down on the origin, for the map editor.
    TopDown,
    /// Flies free of the player on WASD/QE, turned by dragging.
    FreeFly,
}

/// This frame's movement: a world-space XZ direction (unit or zero), turned
/// from the held keys by the camera's yaw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct MoveIntent {
    pub dir: [f32; 2],
    pub run: bool,
    /// Space went down since the intent was last taken.
    pub jump: bool,
}

/// One view's camera and input state.
#[derive(Debug)]
pub struct Controls {
    pub mode: CameraMode,
    pub config: CameraConfig,
    /// Off, keys and the mouse are ignored and the intent is still.
    pub input_enabled: bool,
    /// The point the camera blends toward while the player is in a conversation.
    pub focus: Option<Vec3>,
    follow: Option<Vec3>,
    look: Look,
    rig: Rig,
    intent: MoveIntent,
    /// The yaw `intent` is relative to.
    yaw: f32,
    /// A free-fly drag that started on this view's window.
    dragging: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            mode: CameraMode::default(),
            config: CameraConfig::default(),
            input_enabled: true,
            focus: None,
            follow: None,
            look: Look::default(),
            rig: Rig::default(),
            intent: MoveIntent::default(),
            yaw: 0.0,
            dragging: false,
        }
    }
}

impl Controls {
    /// The followed player's body root and yaw. The first one turns the
    /// camera to face the way the player does.
    pub fn set_follow(&mut self, pos: Vec3, yaw: f32) {
        if self.follow.is_none() {
            self.look.yaw = yaw;
        }
        self.follow = Some(pos);
    }

    /// The intent so far, clearing its jump so one press jumps once.
    pub fn take_intent(&mut self) -> MoveIntent {
        let intent = self.intent;
        self.intent.jump = false;
        intent
    }

    /// The camera yaw the intent is relative to.
    pub fn camera_yaw(&self) -> f32 {
        self.yaw
    }

    fn look_sign(&self) -> f32 {
        if self.config.invert_look { 1.0 } else { -1.0 }
    }
}

/// The views' controls, shareable with callers outside the app.
#[derive(Resource, Clone, Default)]
pub struct ControlsHub(Arc<Mutex<HashMap<ViewId, Controls>>>);

impl ControlsHub {
    fn lock(&self) -> MutexGuard<'_, HashMap<ViewId, Controls>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs `f` on the view's controls, giving the view controls first if it has none.
    pub fn with<R>(&self, id: ViewId, f: impl FnOnce(&mut Controls) -> R) -> R {
        f(self.lock().entry(id).or_default())
    }

    pub fn remove(&self, id: ViewId) {
        self.lock().remove(&id);
    }
}

/// Set by a picking layer when this frame's mouse press landed on a pickable
/// target (e.g. a ping bell), so the press doesn't take pointer lock. Set it
/// in `PreUpdate` (as Bevy's picking observers run) or before
/// [`ControlsSystems`]; the plugin clears it every frame.
#[derive(Resource, Default)]
pub struct PointerClaimed(pub bool);

/// Reads input and moves the views' cameras, before [`SceneSystems`] draws them.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ControlsSystems;

/// Needs Bevy's `InputPlugin` and the views from `officexr_render`.
pub struct ControlsPlugin;

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControlsHub>()
            .init_resource::<PointerClaimed>()
            .configure_sets(Update, ControlsSystems.before(SceneSystems))
            .add_systems(
                Update,
                (pointer_lock, steer).chain().in_set(ControlsSystems),
            );
    }
}

fn window_of(view: &View, primary: Option<Entity>) -> Option<Entity> {
    match view.target() {
        RenderTarget::Window(WindowRef::Primary) => primary,
        RenderTarget::Window(WindowRef::Entity(window)) => Some(*window),
        _ => None,
    }
}

fn is_locked(cursor: &CursorOptions) -> bool {
    cursor.grab_mode != CursorGrabMode::None
}

/// Locks the pointer to a window on a click there while a view on it wants
/// mouse-look, and frees it on Esc or once none does.
#[allow(clippy::too_many_arguments)]
fn pointer_lock(
    hub: Res<ControlsHub>,
    mut claimed: ResMut<PointerClaimed>,
    mut presses: MessageReader<MouseButtonInput>,
    keys: Res<ButtonInput<KeyCode>>,
    views: Query<&View>,
    primary: Option<Single<Entity, With<PrimaryWindow>>>,
    mut cursors: Query<(Entity, &mut CursorOptions)>,
) {
    let primary = primary.map(|p| *p);
    let hub = hub.lock();
    let wants_look: Vec<Entity> = views
        .iter()
        .filter(|v| {
            hub.get(&v.id)
                .is_some_and(|c| c.input_enabled && is_yaw_pitch_mode(c.mode))
        })
        .filter_map(|v| window_of(v, primary))
        .collect();
    let clicked: Vec<Entity> = presses
        .read()
        .filter(|p| p.button == MouseButton::Left && p.state == ButtonState::Pressed)
        .map(|p| p.window)
        .collect();
    let claimed = std::mem::take(&mut claimed.0);
    for (window, mut cursor) in &mut cursors {
        let wanted = wants_look.contains(&window);
        if wanted && !claimed && clicked.contains(&window) {
            // Set even when already locked: the browser frees the lock on its
            // own, and only a changed `CursorOptions` asks for it again.
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        } else if is_locked(&cursor) && (!wanted || keys.just_pressed(KeyCode::Escape)) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
}

/// Mouse input for one window this frame, in CSS px.
#[derive(Default)]
struct WindowInput {
    locked: bool,
    scale_factor: f32,
    motion: Vec2,
    pressed: bool,
    /// DOM wheel deltas (positive = down), each with its zoom factor.
    wheel: Vec<(f32, f32)>,
}

#[allow(clippy::too_many_arguments)]
fn steer(
    mut commands: Commands,
    time: Res<Time>,
    hub: Res<ControlsHub>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut presses: MessageReader<MouseButtonInput>,
    mut wheels: MessageReader<MouseWheel>,
    windows: Query<(Entity, &Window, &CursorOptions)>,
    primary: Option<Single<Entity, With<PrimaryWindow>>>,
    mut views: Query<(Entity, &View, Option<&mut ViewCamera>)>,
) {
    let primary = primary.map(|p| *p);
    let mut inputs: HashMap<Entity, WindowInput> = windows
        .iter()
        .map(|(entity, window, cursor)| {
            let input = WindowInput {
                locked: is_locked(cursor),
                scale_factor: window.scale_factor(),
                motion: motion.delta / window.scale_factor(),
                ..default()
            };
            (entity, input)
        })
        .collect();
    for press in presses.read() {
        if press.button == MouseButton::Left
            && press.state == ButtonState::Pressed
            && let Some(input) = inputs.get_mut(&press.window)
        {
            input.pressed = true;
        }
    }
    for wheel in wheels.read() {
        if let Some(input) = inputs.get_mut(&wheel.window) {
            let delta = dom_wheel_delta(wheel.y, wheel.unit, input.scale_factor);
            input
                .wheel
                .push((delta, wheel_zoom_factor(delta, wheel.unit)));
        }
    }
    let empty = WindowInput::default();
    let dt = time.delta_secs();
    let mut hub = hub.lock();
    for (entity, view, camera) in &mut views {
        let Some(controls) = hub.get_mut(&view.id) else {
            continue;
        };
        let window = window_of(view, primary)
            .and_then(|w| inputs.get(&w))
            .unwrap_or(&empty);
        if controls.input_enabled {
            mouse(controls, window, buttons.pressed(MouseButton::Left));
            if controls.mode == CameraMode::FreeFly {
                controls.rig.free_fly.fly(fly_keys(&keys), dt);
            }
        } else {
            controls.dragging = false;
        }
        controls.yaw = match controls.mode {
            CameraMode::Fixed => fixed_movement_yaw(
                controls.config.fixed.azimuth_deg,
                controls.look.offsets.fixed_azimuth_offset_rad,
                controls.config.fixed.movement_yaw_offset_deg,
            ),
            _ => controls.look.yaw,
        };
        controls.intent = if controls.input_enabled {
            let untaken_jump = controls.intent.jump;
            let mut intent = intent(&keys, controls.yaw);
            intent.jump |= untaken_jump;
            intent
        } else {
            MoveIntent::default()
        };

        let Controls {
            mode,
            config,
            focus,
            follow,
            look,
            rig,
            ..
        } = controls;
        let Some(pose) = rig.step(*mode, config, look, *follow, *focus, dt) else {
            continue;
        };
        match camera {
            Some(mut camera) => camera.0 = pose,
            None => {
                commands.entity(entity).insert(ViewCamera(pose));
            }
        }
    }
}

/// Applies a window's mouse input to a view on it.
fn mouse(controls: &mut Controls, input: &WindowInput, left_held: bool) {
    let mode = controls.mode;
    if input.locked && is_yaw_pitch_mode(mode) {
        let d = input.motion * controls.look_sign() * MOUSE_SENSITIVITY;
        apply_look_delta(mode, d.x, d.y, &mut controls.look);
    }
    if mode == CameraMode::FreeFly {
        controls.dragging = (controls.dragging || input.pressed) && left_held;
        if controls.dragging {
            controls.rig.free_fly.drag(input.motion);
        }
    }
    for &(delta, factor) in &input.wheel {
        if mode == CameraMode::FreeFly {
            controls.rig.free_fly.dolly(delta);
        } else if is_look_mode(mode) {
            apply_zoom(factor, &mut controls.look.offsets);
        }
    }
}

fn axis(keys: &ButtonInput<KeyCode>, plus: &[KeyCode], minus: &[KeyCode]) -> f32 {
    f32::from(u8::from(keys.any_pressed(plus.iter().copied())))
        - f32::from(u8::from(keys.any_pressed(minus.iter().copied())))
}

const SHIFT: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];

/// Held with a key, a shortcut (ctrl+S, cmd+A) rather than movement.
const COMMAND: [KeyCode; 4] = [
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
];

/// WASD or the arrows, turned by `yaw`; shift runs and space jumps.
fn intent(keys: &ButtonInput<KeyCode>, yaw: f32) -> MoveIntent {
    if keys.any_pressed(COMMAND) {
        return MoveIntent::default();
    }
    let fwd = axis(
        keys,
        &[KeyCode::KeyW, KeyCode::ArrowUp],
        &[KeyCode::KeyS, KeyCode::ArrowDown],
    );
    let strafe = axis(
        keys,
        &[KeyCode::KeyD, KeyCode::ArrowRight],
        &[KeyCode::KeyA, KeyCode::ArrowLeft],
    );
    let forward = Vec2::new(-yaw.sin(), -yaw.cos());
    let right = Vec2::new(yaw.cos(), -yaw.sin());
    let dir = (forward * fwd + right * strafe).normalize_or_zero();
    MoveIntent {
        dir: dir.into(),
        run: keys.any_pressed(SHIFT),
        jump: keys.just_pressed(KeyCode::Space),
    }
}

fn fly_keys(keys: &ButtonInput<KeyCode>) -> FlyKeys {
    FlyKeys {
        axes: Vec3::new(
            axis(keys, &[KeyCode::KeyD], &[KeyCode::KeyA]),
            axis(
                keys,
                &[KeyCode::KeyE, KeyCode::Space],
                &[KeyCode::KeyQ, KeyCode::ControlLeft, KeyCode::ControlRight],
            ),
            axis(keys, &[KeyCode::KeyW], &[KeyCode::KeyS]),
        ),
        fast: keys.any_pressed(SHIFT),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input::InputPlugin;
    use bevy::time::TimeUpdateStrategy;
    use officexr_render::SceneCamera;
    use officexr_render::views::{ViewHub, ViewKind, ViewTarget};

    use super::*;

    fn app() -> (App, ViewId) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            InputPlugin,
            officexr_render::views::plugin,
            ControlsPlugin,
        ))
        .init_asset::<Mesh>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
        let id = app
            .world()
            .resource::<ViewHub>()
            .create(ViewKind::World, |_| ViewTarget::PrimaryWindow);
        app.update();
        (app, id)
    }

    fn controls<R>(app: &App, id: ViewId, f: impl FnOnce(&mut Controls) -> R) -> R {
        app.world().resource::<ControlsHub>().with(id, f)
    }

    fn camera(app: &mut App, id: ViewId) -> SceneCamera {
        let mut views = app.world_mut().query::<(&View, &ViewCamera)>();
        let (_, camera) = views.iter(app.world()).find(|(v, _)| v.id == id).unwrap();
        camera.0.clone()
    }

    #[test]
    fn a_camera_config_set_at_runtime_moves_the_camera() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.set_follow(Vec3::ZERO, 0.0));
        app.update();
        assert_eq!(camera(&mut app, id).fov_deg, 50.0);

        let config: CameraConfig =
            serde_json::from_str(r#"{"fixed": {"fov": 70, "azimuthDeg": 0}}"#).unwrap();
        controls(&app, id, |c| c.config = config);
        controls(&app, id, |c| c.mode = CameraMode::TopDown);
        app.update();
        controls(&app, id, |c| c.mode = CameraMode::Fixed);
        app.update();
        let camera = camera(&mut app, id);
        assert_eq!(camera.fov_deg, 70.0);
        // Azimuth 0 puts the camera due north (-z) of the player.
        assert!(camera.position[0].abs() < 1e-3 && camera.position[2] < 0.0);
        let yaw = controls(&app, id, |c| c.camera_yaw());
        assert!((yaw - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn a_view_with_input_disabled_ignores_keys() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.mode = CameraMode::FirstPerson;
            c.set_follow(Vec3::ZERO, 0.0);
        });
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::ShiftLeft);
        app.update();
        let intent = controls(&app, id, Controls::take_intent);
        assert!((Vec2::from(intent.dir) - Vec2::new(0.0, -1.0)).length() < 1e-5);
        assert!(intent.run);

        controls(&app, id, |c| c.input_enabled = false);
        app.update();
        assert_eq!(
            controls(&app, id, Controls::take_intent),
            MoveIntent::default()
        );
    }

    #[test]
    fn a_jump_holds_until_taken_once() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::FirstPerson);
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Space,
                logical_key: bevy::input::keyboard::Key::Space,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        app.update();
        app.update();
        assert!(controls(&app, id, Controls::take_intent).jump);
        assert!(!controls(&app, id, Controls::take_intent).jump);
    }
}
