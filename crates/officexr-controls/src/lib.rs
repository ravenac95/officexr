//! Keyboard, mouse, touch and device-motion input and the gameplay camera modes, per view.
//!
//! Each view with controls has a [`Controls`] entry in the [`ControlsHub`],
//! which callers outside the app (the web API, the native client) feed and
//! read. Every frame the plugin turns held keys into a [`MoveIntent`] relative
//! to the camera's yaw, and the mouse and the followed player into the view's
//! [`ViewCamera`]. The caller steps the sim with the intent and hands the
//! player's new position back with [`Controls::set_follow`].
//!
//! Keys are read from Bevy's `ButtonInput<KeyCode>` and reach every view
//! whose input is enabled; the host feeds it the keys the views should hear
//! (on the web, the whole page's). Pointer lock and mouse-look need the view to
//! draw into a Bevy window; on WebGL2 only the primary view does. A view on an
//! image takes its drags and wheel from the host as [`ViewPointerInput`].

mod config;
pub mod look;
pub mod motion;
mod rig;
pub mod touch;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use bevy::camera::RenderTarget;
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowRef};
use officexr_render::views::{SceneSystems, View, ViewCamera, ViewId};
use serde::{Deserialize, Serialize};

pub use config::{CameraConfig, FixedCamera, OrbitCamera, RotateButton};
use look::{
    Drag, Look, MOUSE_SENSITIVITY, Modifiers, Orbit, apply_look_delta, apply_zoom, dom_wheel_delta,
    drag_for, drag_look_delta, fixed_movement_yaw, is_look_mode, is_yaw_pitch_mode,
    wheel_zoom_factor,
};
use motion::{DeviceReading, MotionPose, look_delta, to_pose};
use rig::{FlyKeys, Rig};
pub use touch::JoystickKind;
use touch::{TouchState, read_touches, spawn_stick};

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
    /// Orbits a target the user pans and zooms, for editors and previews.
    Orbit,
}

/// This frame's movement: a world-space XZ direction (unit or zero), turned
/// from the held keys by the camera's yaw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct MoveIntent {
    pub dir: [f32; 2],
    pub run: bool,
    /// Space went down since the intent was last taken.
    pub jump: bool,
    /// The stick's deflection when it, not the keys, gave `dir`; else 0.
    pub deflection: f32,
}

impl MoveIntent {
    /// Forward and strafe axes (+1 forward, +1 right) turned by the camera's `yaw` into a unit
    /// world-space direction.
    pub fn from_axes(fwd: f32, strafe: f32, run: bool, jump: bool, yaw: f32) -> Self {
        let forward = Vec2::new(-yaw.sin(), -yaw.cos());
        let right = Vec2::new(yaw.cos(), -yaw.sin());
        Self {
            dir: (forward * fwd + right * strafe).normalize_or_zero().into(),
            run,
            jump,
            deflection: 0.0,
        }
    }

    /// As `moveIntent.ts`: held movement keys win over the `stick` (`x` strafe, `y` forward), and
    /// a near-full deflection runs.
    pub fn or_stick(self, stick: Vec2, yaw: f32) -> Self {
        if self.dir != [0.0; 2] || stick == Vec2::ZERO {
            return self;
        }
        let deflection = stick.length();
        let run = self.run || deflection >= RUN_DEFLECTION;
        Self {
            deflection,
            ..Self::from_axes(stick.y, stick.x, run, self.jump, yaw)
        }
    }
}

const RUN_DEFLECTION: f32 = 0.9;

/// One view's camera and input state.
#[derive(Debug)]
pub struct Controls {
    pub mode: CameraMode,
    pub config: CameraConfig,
    /// Off, keys and the mouse are ignored and the intent is still.
    pub input_enabled: bool,
    /// Off, keys are ignored but the mouse still drives the camera, as while
    /// typing in a field beside a preview.
    pub keys_enabled: bool,
    /// The point the camera blends toward while the player is in a conversation.
    pub focus: Option<Vec3>,
    /// The on-screen stick, `x` strafe (right +) and `y` forward (+), within the unit circle. The
    /// Bevy joystick overwrites it each frame unless `joystick` is [`JoystickKind::Dom`].
    pub stick: Vec2,
    pub joystick: JoystickKind,
    /// On, a finger dragging on the view no longer turns it.
    pub look_locked: bool,
    follow: Option<Vec3>,
    look: Look,
    rig: Rig,
    motion: Option<MotionPose>,
    intent: MoveIntent,
    /// The yaw `intent` is relative to.
    yaw: f32,
    /// A drag that started on this view's window, and the button holding it.
    drag: Option<(MouseButton, Drag)>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            mode: CameraMode::default(),
            config: CameraConfig::default(),
            input_enabled: true,
            keys_enabled: true,
            focus: None,
            stick: Vec2::ZERO,
            joystick: JoystickKind::default(),
            look_locked: false,
            follow: None,
            look: Look::default(),
            rig: Rig::default(),
            motion: None,
            intent: MoveIntent::default(),
            yaw: 0.0,
            drag: None,
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

    /// Turns the view by a drag of `delta` px at `sensitivity` radians per px.
    pub fn look_by(&mut self, delta: Vec2, sensitivity: f32) {
        let d = drag_look_delta(delta, sensitivity, self.look_sign());
        apply_look_delta(self.mode, d.x, d.y, &mut self.look);
    }

    /// Turns the view by the device's orientation change since the previous reading. A reading
    /// without an orientation forgets the previous one, so the next starts afresh.
    pub fn orient(&mut self, reading: DeviceReading) {
        let pose = to_pose(reading);
        if let (Some(prev), Some(next)) = (self.motion, pose) {
            let (d_yaw, d_pitch) = look_delta(prev, next);
            apply_look_delta(self.mode, d_yaw, d_pitch, &mut self.look);
        }
        self.motion = pose;
    }

    /// The camera yaw the intent is relative to.
    pub fn camera_yaw(&self) -> f32 {
        self.yaw
    }

    /// Moves the orbit camera at once, without easing. Each frame clamps it
    /// to the config's limits.
    pub fn set_orbit(&mut self, place: impl FnOnce(&mut Orbit)) {
        self.rig.place_orbit(place);
    }

    /// Points the orbit camera at the box from `min` to `max` and backs it
    /// off until the box fits.
    pub fn frame_bounds(&mut self, min: Vec3, max: Vec3) {
        let config = &self.config.orbit;
        self.rig.place_orbit(|orbit| orbit.frame(min, max, config));
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
/// target (e.g. a ping bell), so the press neither takes pointer lock nor
/// starts a drag. Claim only primary-button presses: the right and middle
/// buttons drive the orbit camera wherever they land. Set it in `PreUpdate` (as Bevy's picking observers
/// run) or before [`ControlsSystems`]; the plugin clears it every frame.
#[derive(Resource, Default)]
pub struct PointerClaimed(pub bool);

/// Sent by the host when the platform ended pointer lock on its own (Esc, a
/// tab switch, a refused request): the plugin frees and shows the cursor.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct PointerLockLost;

/// Mouse input for a view that draws into an image rather than a window, which
/// the host reads from wherever it shows the image (a WebGL2 image view's canvas).
#[derive(Message, Clone, Copy, Debug)]
pub struct ViewPointerInput {
    pub view: ViewId,
    pub pointer: ViewPointer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewPointer {
    Press(MouseButton),
    Release(MouseButton),
    /// The pointer moved by this many CSS px.
    Move(Vec2),
    /// A scroll, signed as Bevy's `MouseWheel`, in CSS px or lines.
    Wheel(f32, MouseScrollUnit),
    /// The host lost the pointer mid-press: every button lets go.
    Lost,
}

impl ViewPointer {
    /// The presses and releases that take the DOM's `MouseEvent.buttons` from
    /// `before` to `now`. Read on every pointer event, it catches a button
    /// pressed or let go while another is held, which the DOM reports only as
    /// a `pointermove`.
    pub fn button_changes(before: u16, now: u16) -> impl Iterator<Item = ViewPointer> {
        const BITS: [(u16, MouseButton); 3] = [
            (1, MouseButton::Left),
            (2, MouseButton::Right),
            (4, MouseButton::Middle),
        ];
        BITS.into_iter()
            .filter(move |(bit, _)| (before ^ now) & bit != 0)
            .map(move |(bit, button)| {
                if now & bit != 0 {
                    ViewPointer::Press(button)
                } else {
                    ViewPointer::Release(button)
                }
            })
    }
}

/// Reads input and moves the views' cameras, before [`SceneSystems`] draws them.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ControlsSystems;

/// Needs Bevy's `InputPlugin` and the views from `officexr_render`.
pub struct ControlsPlugin;

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControlsHub>()
            .init_resource::<PointerClaimed>()
            .init_resource::<ShortcutKeys>()
            .init_resource::<WindowInputs>()
            .init_resource::<TouchState>()
            .init_resource::<Turns>()
            .add_message::<PointerLockLost>()
            .add_message::<ViewPointerInput>()
            .configure_sets(Update, ControlsSystems.before(SceneSystems))
            .add_systems(Startup, spawn_stick)
            .add_systems(
                Update,
                (
                    track_shortcuts,
                    desktop_turn,
                    gather_mouse,
                    read_touches,
                    steer,
                )
                    .chain()
                    .in_set(ControlsSystems),
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

/// Held with a key, a shortcut (ctrl+S, cmd+A) rather than movement.
const COMMAND: [KeyCode; 6] = [
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
];

/// Keys that went down with a command modifier held, ignored until they're
/// pressed again without one or released. macOS sends no keyup for a key
/// released while cmd is held, so gating on the modifier being held now would
/// leave such a key moving the player after cmd comes up.
#[derive(Resource, Default)]
struct ShortcutKeys {
    ignored: HashSet<KeyCode>,
    /// Held command modifiers, tracked in event order so a key pressed before
    /// a modifier in the same frame still counts.
    modifiers: HashSet<KeyCode>,
}

fn track_shortcuts(
    mut events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut shortcuts: ResMut<ShortcutKeys>,
) {
    let ShortcutKeys { ignored, modifiers } = &mut *shortcuts;
    for event in events.read() {
        let pressed = event.state == ButtonState::Pressed;
        if COMMAND.contains(&event.key_code) {
            if pressed {
                modifiers.insert(event.key_code);
            } else {
                modifiers.remove(&event.key_code);
            }
        } else if event.repeat {
        } else if pressed && !modifiers.is_empty() {
            ignored.insert(event.key_code);
        } else {
            ignored.remove(&event.key_code);
        }
    }
    // Bevy releases every key when the window loses focus.
    modifiers.retain(|k| keys.pressed(*k));
    ignored.retain(|k| keys.pressed(*k));
}

/// The held keys, less those held for a shortcut.
#[derive(SystemParam)]
struct MovementKeys<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    shortcuts: Res<'w, ShortcutKeys>,
}

impl MovementKeys<'_> {
    fn any(&self, codes: &[KeyCode]) -> bool {
        codes
            .iter()
            .any(|k| self.keys.pressed(*k) && !self.shortcuts.ignored.contains(k))
    }

    fn just_pressed(&self, code: KeyCode) -> bool {
        self.keys.just_pressed(code) && !self.shortcuts.ignored.contains(&code)
    }

    fn axis(&self, plus: &[KeyCode], minus: &[KeyCode]) -> f32 {
        f32::from(u8::from(self.any(plus))) - f32::from(u8::from(self.any(minus)))
    }
}

/// CSS px a left drag travels before it turns the view; `desktopLook.ts`'s
/// `LOOK_DRAG_THRESHOLD_PX`, which tells a click from a drag on the page side.
pub const LOOK_DRAG_THRESHOLD_PX: f32 = 4.0;

/// One window's mouse-look: the right button held, or a left drag past
/// [`LOOK_DRAG_THRESHOLD_PX`], turns the view until every button lets go.
#[derive(Default)]
struct Turn {
    held: Vec<MouseButton>,
    /// CSS px the mouse moved since the first button went down.
    travel: Vec2,
    turning: bool,
}

/// Each window's turn, while a view on it wants mouse-look.
#[derive(Resource, Default)]
struct Turns(HashMap<Entity, Turn>);

impl Turns {
    fn turning(&self, window: Entity) -> bool {
        self.0.get(&window).is_some_and(|t| t.turning)
    }
}

/// Turns the view while the right button is held or the left one drags, on a
/// window a yaw-pitch view wants mouse-look on; the cursor is free otherwise.
/// The pointer is locked only while turning, and Esc or a lost lock frees it
/// without ending the turn, which then reads the unlocked motion.
// Each parameter is one input to the turn; bundling them would only rename them.
#[allow(clippy::too_many_arguments)]
fn desktop_turn(
    hub: Res<ControlsHub>,
    claimed: Res<PointerClaimed>,
    mut turns: ResMut<Turns>,
    mut lost: MessageReader<PointerLockLost>,
    mut presses: MessageReader<MouseButtonInput>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    views: Query<&View>,
    primary: Option<Single<Entity, With<PrimaryWindow>>>,
    mut windows: Query<(Entity, &Window, &mut CursorOptions)>,
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
    turns.0.retain(|window, _| wants_look.contains(window));
    let was_turning: HashSet<Entity> = turns
        .0
        .iter()
        .filter_map(|(window, t)| t.turning.then_some(*window))
        .collect();
    for press in presses.read() {
        let button = press.button;
        if !matches!(button, MouseButton::Left | MouseButton::Right)
            || !wants_look.contains(&press.window)
        {
            continue;
        }
        let turn = turns.0.entry(press.window).or_default();
        if press.state == ButtonState::Released {
            turn.held.retain(|b| *b != button);
        } else if !(claimed.0 && button == MouseButton::Left) {
            if turn.held.is_empty() {
                turn.travel = Vec2::ZERO;
            }
            if !turn.held.contains(&button) {
                turn.held.push(button);
            }
            turn.turning |= button == MouseButton::Right;
        }
    }
    let lost = lost.read().count() > 0;
    let freed = lost || keys.just_pressed(KeyCode::Escape);
    for (entity, window, mut cursor) in &mut windows {
        if let Some(turn) = turns.0.get_mut(&entity) {
            // A release outside the window may never arrive.
            turn.held.retain(|b| buttons.pressed(*b));
            if !turn.held.is_empty() {
                turn.travel += motion.delta / window.scale_factor();
            }
            turn.turning = !turn.held.is_empty()
                && (turn.turning
                    || (turn.held.contains(&MouseButton::Left)
                        && turn.travel.length() >= LOOK_DRAG_THRESHOLD_PX));
        }
        let turning = turns.turning(entity);
        if turning && !was_turning.contains(&entity) {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        } else if is_locked(&cursor) && (freed || !turning) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
}

/// Mouse input for one window this frame, in CSS px.
#[derive(Default)]
struct WindowInput {
    /// Mouse-look is turning the view on this window.
    turning: bool,
    scale_factor: f32,
    motion: Vec2,
    /// Buttons that went down here, on nothing a picker claimed.
    pressed: Vec<MouseButton>,
    held: Vec<MouseButton>,
    /// DOM wheel deltas (positive = down), each with its zoom factor.
    wheel: Vec<(f32, f32)>,
}

/// This frame's mouse input per window, and per view the host feeds.
#[derive(Resource, Default)]
struct WindowInputs {
    windows: HashMap<Entity, WindowInput>,
    views: HashMap<ViewId, WindowInput>,
    /// Each fed view's buttons held now, carried from frame to frame.
    view_buttons: HashMap<ViewId, Vec<MouseButton>>,
}

impl WindowInputs {
    fn of(&self, view: &View, primary: Option<Entity>) -> Option<&WindowInput> {
        match window_of(view, primary) {
            Some(window) => self.windows.get(&window),
            None => self.views.get(&view.id),
        }
    }
}

// Each parameter is one input source; bundling them would only rename them.
#[allow(clippy::too_many_arguments)]
fn gather_mouse(
    mut inputs: ResMut<WindowInputs>,
    mut claimed: ResMut<PointerClaimed>,
    turns: Res<Turns>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut presses: MessageReader<MouseButtonInput>,
    mut wheels: MessageReader<MouseWheel>,
    mut fed: MessageReader<ViewPointerInput>,
    windows: Query<(Entity, &Window)>,
    views: Query<&View>,
) {
    let claimed = std::mem::take(&mut claimed.0);
    let WindowInputs {
        views: view_inputs,
        view_buttons,
        ..
    } = &mut *inputs;
    gather_view_pointers(view_inputs, view_buttons, &mut fed, claimed, &views);
    let held: Vec<MouseButton> = buttons.get_pressed().copied().collect();
    inputs.windows = windows
        .iter()
        .map(|(entity, window)| {
            let input = WindowInput {
                turning: turns.turning(entity),
                scale_factor: window.scale_factor(),
                motion: motion.delta / window.scale_factor(),
                held: held.clone(),
                ..default()
            };
            (entity, input)
        })
        .collect();
    for press in presses.read() {
        if press.state == ButtonState::Pressed
            && !claimed
            && let Some(input) = inputs.windows.get_mut(&press.window)
        {
            input.pressed.push(press.button);
        }
    }
    for wheel in wheels.read() {
        if let Some(input) = inputs.windows.get_mut(&wheel.window) {
            let delta = dom_wheel_delta(wheel.y, wheel.unit, input.scale_factor);
            input
                .wheel
                .push((delta, wheel_zoom_factor(delta, wheel.unit)));
        }
    }
}

/// This frame's input for each view the host feeds. The host's events can
/// hold a whole drag, press to release, when a frame is slow: a button let go
/// this frame still counts as held for it, and only moves made while a button
/// was down count as drag motion.
fn gather_view_pointers(
    inputs: &mut HashMap<ViewId, WindowInput>,
    buttons: &mut HashMap<ViewId, Vec<MouseButton>>,
    fed: &mut MessageReader<ViewPointerInput>,
    claimed: bool,
    views: &Query<&View>,
) {
    buttons.retain(|id, _| views.iter().any(|v| v.id == *id));
    inputs.clear();
    for ViewPointerInput { view, pointer } in fed.read().copied() {
        let down = buttons.entry(view).or_default();
        let input = inputs.entry(view).or_insert_with(|| WindowInput {
            scale_factor: 1.0,
            held: down.clone(),
            ..default()
        });
        match pointer {
            ViewPointer::Press(button) => {
                if !claimed {
                    input.pressed.push(button);
                }
                for held in [&mut input.held, &mut *down] {
                    if !held.contains(&button) {
                        held.push(button);
                    }
                }
            }
            ViewPointer::Release(button) => down.retain(|b| *b != button),
            ViewPointer::Move(delta) if !down.is_empty() => input.motion += delta,
            ViewPointer::Move(_) => {}
            ViewPointer::Wheel(y, unit) => {
                let delta = dom_wheel_delta(y, unit, 1.0);
                input.wheel.push((delta, wheel_zoom_factor(delta, unit)));
            }
            ViewPointer::Lost => down.clear(),
        }
    }
    // A view with no events this frame keeps its buttons held.
    for (view, down) in buttons.iter() {
        inputs.entry(*view).or_insert_with(|| WindowInput {
            scale_factor: 1.0,
            held: down.clone(),
            ..default()
        });
    }
}

/// Turns each view's input into its intent and its camera.
fn steer(
    mut commands: Commands,
    time: Res<Time>,
    hub: Res<ControlsHub>,
    keys: MovementKeys,
    inputs: Res<WindowInputs>,
    primary: Option<Single<Entity, With<PrimaryWindow>>>,
    mut views: Query<(Entity, &View, Option<&mut ViewCamera>)>,
) {
    let primary = primary.map(|p| *p);
    let empty = WindowInput::default();
    let dt = time.delta_secs();
    let mut hub = hub.lock();
    for (entity, view, camera) in &mut views {
        let Some(controls) = hub.get_mut(&view.id) else {
            continue;
        };
        let window = inputs.of(view, primary).unwrap_or(&empty);
        let keys_on = controls.input_enabled && controls.keys_enabled;
        if controls.input_enabled {
            let held = Modifiers {
                shift: keys_on && keys.any(&SHIFT),
                ctrl: keys_on && keys.any(&CTRL),
            };
            mouse(controls, window, held);
        } else {
            controls.drag = None;
        }
        if keys_on {
            match controls.mode {
                CameraMode::FreeFly => controls.rig.free_fly.fly(fly_keys(&keys), dt),
                CameraMode::Orbit if controls.config.orbit.move_keys => {
                    controls.rig.orbit_goal.fly(orbit_keys(&keys), dt);
                }
                _ => {}
            }
        }
        controls.yaw = match controls.mode {
            CameraMode::Fixed => fixed_movement_yaw(
                controls.config.fixed.azimuth_deg,
                controls.look.offsets.fixed_azimuth_offset_rad,
                controls.config.fixed.movement_yaw_offset_deg,
            ),
            CameraMode::Orbit => controls.rig.orbit_goal.azimuth,
            _ => controls.look.yaw,
        };
        // The orbit's move keys are WASD too: moving the player as well would move both.
        let keys_move_player =
            !(controls.mode == CameraMode::Orbit && controls.config.orbit.move_keys);
        controls.intent = if keys_on && keys_move_player {
            let untaken_jump = controls.intent.jump;
            let mut intent = intent(&keys, controls.yaw).or_stick(controls.stick, controls.yaw);
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
fn mouse(controls: &mut Controls, input: &WindowInput, held: Modifiers) {
    let mode = controls.mode;
    if input.turning && is_yaw_pitch_mode(mode) {
        controls.look_by(input.motion, MOUSE_SENSITIVITY);
    }
    let orbit = &controls.config.orbit;
    let rig = &mut controls.rig;
    controls.drag = controls
        .drag
        .or_else(|| {
            input
                .pressed
                .iter()
                .find_map(|&b| drag_for(mode, b, held, orbit).map(|drag| (b, drag)))
        })
        .filter(|(button, _)| input.held.contains(button));
    match (mode, controls.drag.map(|(_, drag)| drag)) {
        (CameraMode::FreeFly, Some(Drag::Turn)) => rig.free_fly.drag(input.motion),
        (CameraMode::Orbit, Some(Drag::Rotate)) => rig.orbit_goal.rotate(input.motion, orbit),
        (CameraMode::Orbit, Some(Drag::Pan)) => rig.orbit_goal.pan(input.motion),
        (CameraMode::Orbit, Some(Drag::Dolly)) => {
            rig.zoom_orbit(|o| o.dolly(input.motion.y, orbit));
        }
        _ => {}
    }
    for &(delta, factor) in &input.wheel {
        match mode {
            CameraMode::FreeFly => rig.free_fly.dolly(delta),
            CameraMode::Orbit => rig.zoom_orbit(|o| o.zoom(factor, orbit)),
            _ if is_look_mode(mode) => apply_zoom(factor, &mut controls.look.offsets),
            _ => {}
        }
    }
}

const SHIFT: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];
/// Ctrl, or cmd on macOS.
const CTRL: [KeyCode; 4] = [
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
];

/// WASD or the arrows, turned by `yaw`; shift runs and space jumps.
fn intent(keys: &MovementKeys, yaw: f32) -> MoveIntent {
    let fwd = keys.axis(
        &[KeyCode::KeyW, KeyCode::ArrowUp],
        &[KeyCode::KeyS, KeyCode::ArrowDown],
    );
    let strafe = keys.axis(
        &[KeyCode::KeyD, KeyCode::ArrowRight],
        &[KeyCode::KeyA, KeyCode::ArrowLeft],
    );
    MoveIntent::from_axes(
        fwd,
        strafe,
        keys.any(&SHIFT),
        keys.just_pressed(KeyCode::Space),
        yaw,
    )
}

/// The map editor's: WASD across the ground, Q/E up and down, and nothing
/// else, so ctrl held for a shortcut doesn't sink the target.
fn orbit_keys(keys: &MovementKeys) -> Vec3 {
    Vec3::new(
        keys.axis(&[KeyCode::KeyD], &[KeyCode::KeyA]),
        keys.axis(&[KeyCode::KeyE], &[KeyCode::KeyQ]),
        keys.axis(&[KeyCode::KeyW], &[KeyCode::KeyS]),
    )
}

fn fly_keys(keys: &MovementKeys) -> FlyKeys {
    FlyKeys {
        axes: Vec3::new(
            keys.axis(&[KeyCode::KeyD], &[KeyCode::KeyA]),
            keys.axis(
                &[KeyCode::KeyE, KeyCode::Space],
                &[KeyCode::KeyQ, KeyCode::ControlLeft, KeyCode::ControlRight],
            ),
            keys.axis(&[KeyCode::KeyW], &[KeyCode::KeyS]),
        ),
        fast: keys.any(&SHIFT),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input::InputPlugin;
    use bevy::input::keyboard::Key;
    use bevy::input::mouse::{MouseMotion, MouseScrollUnit};
    use bevy::input::touch::{TouchInput, TouchPhase};
    use bevy::time::TimeUpdateStrategy;
    use bevy::window::WindowResolution;
    use officexr_render::SceneCamera;
    use officexr_render::views::{ViewHub, ViewKind, ViewTarget};

    use super::look::ViewOffsets;
    use super::*;

    const SCALE: f32 = 2.0;

    /// An app with one world view on a primary window at `SCALE`.
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
        app.world_mut().spawn((
            Window {
                resolution: WindowResolution::default().with_scale_factor_override(SCALE),
                ..default()
            },
            PrimaryWindow,
        ));
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

    fn window(app: &mut App) -> Entity {
        app.world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap()
    }

    fn cursor(app: &mut App) -> CursorOptions {
        let window = window(app);
        app.world().get::<CursorOptions>(window).unwrap().clone()
    }

    fn click(app: &mut App) {
        press(app, MouseButton::Left, ButtonState::Pressed);
    }

    fn press(app: &mut App, button: MouseButton, state: ButtonState) {
        let window = window(app);
        app.world_mut().write_message(MouseButtonInput {
            button,
            state,
            window,
        });
    }

    fn key(app: &mut App, key_code: KeyCode, state: ButtonState) {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }

    /// Counts the frames `CursorOptions` changed in, as bevy_winit sees them.
    #[derive(Resource, Default)]
    struct CursorChanges(u32);

    fn count_cursor_changes(
        changed: Query<(), Changed<CursorOptions>>,
        mut changes: ResMut<CursorChanges>,
    ) {
        changes.0 += u32::try_from(changed.iter().count()).unwrap();
    }

    fn motion(app: &mut App, delta: Vec2) {
        app.world_mut().write_message(MouseMotion { delta });
    }

    #[test]
    fn a_right_press_locks_the_pointer_until_released_and_a_lost_lock_shows_the_cursor_again() {
        let (mut app, id) = app();
        app.init_resource::<CursorChanges>()
            .add_systems(Last, count_cursor_changes);
        controls(&app, id, |c| c.mode = CameraMode::FirstPerson);

        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        app.update();
        let locked = cursor(&mut app);
        assert_eq!(locked.grab_mode, CursorGrabMode::Locked);
        assert!(!locked.visible);

        let before = app.world().resource::<CursorChanges>().0;
        app.world_mut().write_message(PointerLockLost);
        app.update();
        let freed = cursor(&mut app);
        assert_eq!(freed.grab_mode, CursorGrabMode::None);
        assert!(freed.visible);
        assert!(app.world().resource::<CursorChanges>().0 > before);

        // The turn outlives the lost lock, on the unlocked motion.
        let yaw = controls(&app, id, |c| c.look.yaw);
        motion(&mut app, Vec2::new(40.0, 0.0));
        app.update();
        assert_ne!(controls(&app, id, |c| c.look.yaw), yaw);

        press(&mut app, MouseButton::Right, ButtonState::Released);
        app.update();
        let yaw = controls(&app, id, |c| c.look.yaw);
        motion(&mut app, Vec2::new(40.0, 0.0));
        app.update();
        assert_eq!(controls(&app, id, |c| c.look.yaw), yaw);
    }

    #[test]
    fn a_left_click_leaves_the_cursor_free_and_a_left_drag_turns_until_released() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.mode = CameraMode::FirstPerson;
            c.set_follow(Vec3::ZERO, 0.0);
        });
        click(&mut app);
        app.update();
        assert_eq!(cursor(&mut app).grab_mode, CursorGrabMode::None);

        // Under the threshold in CSS px: still a click.
        motion(
            &mut app,
            Vec2::new(LOOK_DRAG_THRESHOLD_PX * SCALE * 0.5, 0.0),
        );
        app.update();
        assert_eq!(controls(&app, id, |c| c.look.yaw), 0.0);
        assert_eq!(cursor(&mut app).grab_mode, CursorGrabMode::None);

        motion(&mut app, Vec2::new(LOOK_DRAG_THRESHOLD_PX * SCALE, 0.0));
        app.update();
        assert_eq!(cursor(&mut app).grab_mode, CursorGrabMode::Locked);
        assert_ne!(controls(&app, id, |c| c.look.yaw), 0.0);

        press(&mut app, MouseButton::Left, ButtonState::Released);
        app.update();
        assert_eq!(cursor(&mut app).grab_mode, CursorGrabMode::None);
    }

    #[test]
    fn esc_frees_the_pointer_and_a_claimed_click_doesnt_turn() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::FirstPerson);
        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        app.update();
        key(&mut app, KeyCode::Escape, ButtonState::Pressed);
        app.update();
        assert_eq!(cursor(&mut app).grab_mode, CursorGrabMode::None);
        press(&mut app, MouseButton::Right, ButtonState::Released);
        app.update();

        app.world_mut().resource_mut::<PointerClaimed>().0 = true;
        click(&mut app);
        app.update();
        motion(&mut app, Vec2::new(100.0, 0.0));
        app.update();
        assert_eq!(cursor(&mut app).grab_mode, CursorGrabMode::None);
        assert_eq!(controls(&app, id, |c| c.look.yaw), 0.0);
        assert!(!app.world().resource::<PointerClaimed>().0);
    }

    #[test]
    fn mouse_look_is_in_css_px() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.mode = CameraMode::FirstPerson;
            c.set_follow(Vec3::ZERO, 0.0);
        });
        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        app.update();
        let mut expected = controls(&app, id, |c| c.look);
        motion(&mut app, Vec2::new(40.0, 20.0));
        app.update();
        let d = Vec2::new(40.0, 20.0) / SCALE * -MOUSE_SENSITIVITY;
        apply_look_delta(CameraMode::FirstPerson, d.x, d.y, &mut expected);
        assert_eq!(controls(&app, id, |c| c.look), expected);
        assert_ne!(expected.yaw, 0.0);
    }

    #[test]
    fn the_wheel_zooms_a_view_on_its_window() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::FirstPerson);
        let window = window(&mut app);
        let wheel = |window| MouseWheel {
            unit: MouseScrollUnit::Pixel,
            x: 0.0,
            y: -100.0,
            window,
            phase: TouchPhase::Moved,
        };
        app.world_mut().write_message(wheel(Entity::PLACEHOLDER));
        app.update();
        assert_eq!(controls(&app, id, |c| c.look.offsets.zoom), 1.0);

        app.world_mut().write_message(wheel(window));
        app.update();
        let delta = dom_wheel_delta(-100.0, MouseScrollUnit::Pixel, SCALE);
        let mut expected = ViewOffsets::default();
        apply_zoom(
            wheel_zoom_factor(delta, MouseScrollUnit::Pixel),
            &mut expected,
        );
        assert_eq!(controls(&app, id, |c| c.look.offsets.zoom), expected.zoom);
        assert_ne!(expected.zoom, 1.0);
    }

    #[test]
    fn a_key_pressed_for_a_shortcut_stays_ignored_until_pressed_again() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::FirstPerson);
        let moving = |app: &App| controls(app, id, Controls::take_intent).dir != [0.0, 0.0];

        key(&mut app, KeyCode::KeyW, ButtonState::Pressed);
        key(&mut app, KeyCode::SuperLeft, ButtonState::Pressed);
        key(&mut app, KeyCode::KeyA, ButtonState::Pressed);
        app.update();
        // W went down before cmd, so it still moves; A, cmd+A, doesn't.
        let intent = controls(&app, id, Controls::take_intent);
        assert!((Vec2::from(intent.dir) - Vec2::new(0.0, -1.0)).length() < 1e-5);
        key(&mut app, KeyCode::KeyW, ButtonState::Released);

        // macOS sends no keyup for A, so it stays held after cmd comes up.
        key(&mut app, KeyCode::SuperLeft, ButtonState::Released);
        app.update();
        assert!(!moving(&app));

        key(&mut app, KeyCode::KeyA, ButtonState::Pressed);
        app.update();
        assert!(moving(&app));

        key(&mut app, KeyCode::AltLeft, ButtonState::Pressed);
        key(&mut app, KeyCode::KeyD, ButtonState::Pressed);
        key(&mut app, KeyCode::KeyA, ButtonState::Released);
        app.update();
        assert!(!moving(&app));
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

    #[test]
    fn an_orbit_drag_rotates_or_pans_until_released_unless_a_picker_claimed_it() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::Orbit);
        let orbit = |app: &App| controls(app, id, |c| c.rig.orbit_goal);
        let motion = |app: &mut App| {
            app.world_mut().write_message(MouseMotion {
                delta: Vec2::new(40.0, 0.0),
            });
        };

        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        motion(&mut app);
        app.update();
        let mut expected = look::Orbit::default();
        expected.rotate(Vec2::new(40.0 / SCALE, 0.0), &OrbitCamera::default());
        assert_eq!(orbit(&app), expected);
        let yaw = controls(&app, id, |c| c.camera_yaw());
        assert!((yaw - expected.azimuth).abs() < 1e-6);

        press(&mut app, MouseButton::Right, ButtonState::Released);
        motion(&mut app);
        app.update();
        assert_eq!(orbit(&app), expected);

        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ShiftLeft);
        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        motion(&mut app);
        app.update();
        let before = expected;
        expected.pan(Vec2::new(40.0 / SCALE, 0.0));
        assert_eq!(orbit(&app), expected);
        assert_ne!(expected.target, before.target);
        press(&mut app, MouseButton::Right, ButtonState::Released);
        app.update();

        app.world_mut().resource_mut::<PointerClaimed>().0 = true;
        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        motion(&mut app);
        app.update();
        assert_eq!(orbit(&app), expected);
    }

    #[test]
    fn an_image_view_orbits_on_the_pointer_the_host_feeds_it_and_ignores_the_windows() {
        let (mut app, primary) = app();
        app.init_asset::<Image>();
        let id = app
            .world()
            .resource::<ViewHub>()
            .create(ViewKind::Preview, |_| ViewTarget::Image(UVec2::new(8, 8)));
        app.update();
        for view in [primary, id] {
            controls(&app, view, |c| c.mode = CameraMode::Orbit);
        }
        let orbit = |app: &App, view| controls(app, view, |c| c.rig.orbit_goal);
        let feed = |app: &mut App, pointer| {
            app.world_mut()
                .write_message(ViewPointerInput { view: id, pointer });
        };

        feed(&mut app, ViewPointer::Press(MouseButton::Right));
        feed(&mut app, ViewPointer::Move(Vec2::new(30.0, 0.0)));
        app.update();
        let mut expected = look::Orbit::default();
        expected.rotate(Vec2::new(30.0, 0.0), &OrbitCamera::default());
        assert_eq!(orbit(&app, id), expected);
        assert_eq!(orbit(&app, primary), look::Orbit::default());

        // Held across frames, until the host loses the pointer.
        feed(&mut app, ViewPointer::Move(Vec2::new(30.0, 0.0)));
        app.update();
        expected.rotate(Vec2::new(30.0, 0.0), &OrbitCamera::default());
        assert_eq!(orbit(&app, id), expected);
        feed(&mut app, ViewPointer::Lost);
        feed(&mut app, ViewPointer::Move(Vec2::new(30.0, 0.0)));
        app.update();
        assert_eq!(orbit(&app, id), expected);

        // The window's own mouse reaches only the window's view.
        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        app.world_mut().write_message(MouseMotion {
            delta: Vec2::new(40.0, 0.0),
        });
        app.update();
        assert_eq!(orbit(&app, id), expected);
        assert_ne!(orbit(&app, primary), look::Orbit::default());

        feed(&mut app, ViewPointer::Wheel(1.0, MouseScrollUnit::Line));
        app.update();
        assert!(orbit(&app, id).distance < expected.distance);
    }

    #[test]
    fn a_drag_that_starts_and_ends_within_one_frame_still_orbits_an_image_view() {
        let (mut app, _) = app();
        app.init_asset::<Image>();
        let id = app
            .world()
            .resource::<ViewHub>()
            .create(ViewKind::Preview, |_| ViewTarget::Image(UVec2::new(8, 8)));
        app.update();
        controls(&app, id, |c| c.mode = CameraMode::Orbit);
        let orbit = |app: &App| controls(app, id, |c| c.rig.orbit_goal);
        let feed = |app: &mut App, pointer| {
            app.world_mut()
                .write_message(ViewPointerInput { view: id, pointer });
        };

        // A slow frame: the whole drag arrives before the app sees any of it.
        feed(&mut app, ViewPointer::Press(MouseButton::Right));
        feed(&mut app, ViewPointer::Move(Vec2::new(30.0, 0.0)));
        feed(&mut app, ViewPointer::Release(MouseButton::Right));
        // Moved after letting go: not part of the drag.
        feed(&mut app, ViewPointer::Move(Vec2::new(500.0, 0.0)));
        app.update();
        let mut expected = look::Orbit::default();
        expected.rotate(Vec2::new(30.0, 0.0), &OrbitCamera::default());
        assert_eq!(orbit(&app), expected);

        // The button is let go by the next frame.
        feed(&mut app, ViewPointer::Move(Vec2::new(30.0, 0.0)));
        app.update();
        assert_eq!(orbit(&app), expected);
    }

    #[test]
    fn dom_button_masks_turn_into_presses_and_releases_even_when_chorded() {
        use MouseButton::{Left, Right};
        let changes = |before, now| ViewPointer::button_changes(before, now).collect::<Vec<_>>();
        // Left-drag, then right down, left up, right up: the middle two arrive as moves.
        assert_eq!(changes(0, 1), [ViewPointer::Press(Left)]);
        assert_eq!(changes(1, 3), [ViewPointer::Press(Right)]);
        assert_eq!(changes(3, 2), [ViewPointer::Release(Left)]);
        assert_eq!(changes(2, 0), [ViewPointer::Release(Right)]);
        assert_eq!(changes(3, 3), []);
        assert_eq!(
            changes(0, 5),
            [
                ViewPointer::Press(Left),
                ViewPointer::Press(MouseButton::Middle)
            ]
        );
    }

    #[test]
    fn a_view_with_keys_off_still_orbits_on_the_mouse() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.mode = CameraMode::Orbit;
            c.keys_enabled = false;
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        press(&mut app, MouseButton::Right, ButtonState::Pressed);
        app.world_mut().write_message(MouseMotion {
            delta: Vec2::new(40.0, 0.0),
        });
        app.update();
        assert_ne!(
            controls(&app, id, |c| c.rig.orbit_goal),
            look::Orbit::default()
        );
        assert_eq!(
            controls(&app, id, Controls::take_intent),
            MoveIntent::default()
        );
    }

    #[test]
    fn the_wheel_zooms_the_orbit_and_frame_bounds_snaps_it() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::Orbit);
        let window = window(&mut app);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Pixel,
            x: 0.0,
            y: -200.0,
            window,
            phase: TouchPhase::Moved,
        });
        app.update();
        // -200 device px at 2x is a 100 CSS px wheel down.
        let distance = controls(&app, id, |c| c.rig.orbit.distance);
        assert!((distance - 16.0 * 0.1f32.exp()).abs() < 1e-3);

        controls(&app, id, |c| {
            c.frame_bounds(Vec3::new(9.0, 0.0, 9.0), Vec3::new(11.0, 2.0, 11.0));
        });
        app.update();
        let camera = camera(&mut app, id);
        assert!((Vec3::from(camera.look_at) - Vec3::new(10.0, 1.0, 10.0)).length() < 1e-4);
    }

    #[test]
    fn set_orbit_places_the_editor_camera_at_once_within_the_limits() {
        let (mut app, id) = app();
        let (target, eye) = (Vec3::new(0.0, 0.5, 0.0), Vec3::new(2.5, 2.0, 2.5));
        controls(&app, id, |c| {
            c.mode = CameraMode::Orbit;
            c.config.orbit = OrbitCamera {
                min_distance: 1.5,
                max_distance: 40.0,
                damping_secs: 0.33,
                ..OrbitCamera::default()
            };
            c.set_orbit(|orbit| *orbit = Orbit::from_eye(target, eye));
        });
        app.update();
        let camera = camera(&mut app, id);
        assert!((Vec3::from(camera.position) - eye).length() < 1e-4);
        assert!((Vec3::from(camera.look_at) - target).length() < 1e-4);

        let orbit = |app: &App| controls(app, id, |c| c.rig.orbit);
        controls(&app, id, |c| {
            c.set_orbit(|orbit| {
                orbit.elevation = 0.7;
                orbit.distance = 60.0;
            });
        });
        app.update();
        assert_eq!(orbit(&app).target, target);
        assert_eq!((orbit(&app).elevation, orbit(&app).distance), (0.7, 40.0));

        // The map editor's pose, set before its limits: the frame clamps to
        // the limits in force by then.
        controls(&app, id, |c| {
            c.set_orbit(|orbit| orbit.distance = 60.0);
            c.config.orbit = OrbitCamera {
                min_distance: 4.0,
                max_distance: 500.0,
                ..OrbitCamera::default()
            };
        });
        app.update();
        assert_eq!(orbit(&app).distance, 60.0);
    }

    #[test]
    fn orbit_move_keys_ignore_ctrl_and_space_and_leave_the_player_still() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.mode = CameraMode::Orbit;
            c.config.orbit.move_keys = true;
        });
        let target = |app: &App| controls(app, id, |c| c.rig.orbit_goal.target);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::Space);
        app.update();
        assert_eq!(target(&app), look::Orbit::default().target);

        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::KeyE);
        app.update();
        let moved = target(&app) - look::Orbit::default().target;
        assert!(moved.y > 0.0 && Vec2::new(moved.x, moved.z).length() > 0.0);
        assert_eq!(
            controls(&app, id, Controls::take_intent),
            MoveIntent::default()
        );
    }

    #[test]
    fn orbit_zoom_skips_the_damping_as_drei_does() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.mode = CameraMode::Orbit;
            c.config.orbit.damping_secs = 0.33;
        });
        let window = window(&mut app);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Pixel,
            x: 0.0,
            y: -200.0,
            window,
            phase: TouchPhase::Moved,
        });
        app.update();
        let (shown, goal) = controls(&app, id, |c| (c.rig.orbit, c.rig.orbit_goal));
        assert_eq!(shown.distance, goal.distance);
        assert_ne!(goal.distance, 16.0);
    }

    #[test]
    fn keys_win_over_the_stick() {
        let keys = MoveIntent {
            dir: [0.0, -1.0],
            ..default()
        };
        assert_eq!(keys.or_stick(Vec2::new(1.0, 0.0), 0.0), keys);
    }

    #[test]
    fn the_stick_steers_relative_to_the_camera_and_runs_near_full_deflection() {
        let walk = MoveIntent::default().or_stick(Vec2::new(0.0, 0.4), 0.0);
        assert_eq!(
            (walk.dir, walk.run, walk.deflection),
            ([0.0, -1.0], false, 0.4)
        );
        assert!(
            MoveIntent::default()
                .or_stick(Vec2::new(0.0, 0.95), 0.0)
                .run
        );
    }

    #[test]
    fn the_dom_joysticks_stick_walks_the_player() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.joystick = JoystickKind::Dom;
            c.stick = Vec2::new(1.0, 0.0);
        });
        app.update();
        let intent = controls(&app, id, |c| c.take_intent());
        assert!((Vec2::from(intent.dir).length() - 1.0).abs() < 1e-6);
        assert_eq!(intent.deflection, 1.0);
    }

    #[test]
    fn device_motion_turns_the_view_from_the_second_reading() {
        let reading = |alpha| motion::DeviceReading {
            alpha: Some(alpha),
            beta: Some(90.0),
            gamma: Some(0.0),
        };
        let mut c = Controls {
            mode: CameraMode::FirstPerson,
            ..default()
        };
        c.orient(reading(0.0));
        assert_eq!(c.look.yaw, 0.0);
        c.orient(reading(10.0));
        assert!((c.look.yaw - 10f32.to_radians()).abs() < 1e-5);
        c.orient(motion::DeviceReading::default());
        c.orient(reading(20.0));
        assert!((c.look.yaw - 10f32.to_radians()).abs() < 1e-5);
    }

    fn touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, at: Vec2) {
        app.world_mut().write_message(TouchInput {
            phase,
            position: at,
            window,
            force: None,
            id,
        });
    }

    #[test]
    fn one_finger_looks_two_pinch_and_the_stick_finger_does_neither() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::FirstPerson);
        let window = window(&mut app);
        let size = app.world().get::<Window>(window).unwrap().size();
        let stick = Vec2::new(12.0 + 55.0, size.y - 12.0 - 55.0);
        touch(&mut app, window, 0, TouchPhase::Started, stick);
        touch(
            &mut app,
            window,
            1,
            TouchPhase::Started,
            Vec2::new(400.0, 200.0),
        );
        app.update();
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Moved,
            stick + Vec2::new(32.0, 0.0),
        );
        touch(
            &mut app,
            window,
            1,
            TouchPhase::Moved,
            Vec2::new(410.0, 200.0),
        );
        app.update();
        let (yaw, stick_x) = controls(&app, id, |c| (c.look.yaw, c.stick.x));
        assert!((yaw - -0.05).abs() < 1e-6);
        assert_eq!(stick_x, 1.0);

        touch(
            &mut app,
            window,
            2,
            TouchPhase::Started,
            Vec2::new(510.0, 200.0),
        );
        app.update();
        touch(
            &mut app,
            window,
            2,
            TouchPhase::Moved,
            Vec2::new(610.0, 200.0),
        );
        app.update();
        let (zoom, after) = controls(&app, id, |c| (c.look.offsets.zoom, c.look.yaw));
        assert!((zoom - 2.0).abs() < 1e-5);
        assert_eq!(after, yaw);
    }

    #[test]
    fn a_locked_look_ignores_a_dragging_finger() {
        let (mut app, id) = app();
        controls(&app, id, |c| {
            c.mode = CameraMode::FirstPerson;
            c.look_locked = true;
        });
        let window = window(&mut app);
        let at = Vec2::new(400.0, 200.0);
        touch(&mut app, window, 1, TouchPhase::Started, at);
        app.update();
        touch(
            &mut app,
            window,
            1,
            TouchPhase::Moved,
            at + Vec2::new(10.0, 0.0),
        );
        app.update();
        assert_eq!(controls(&app, id, |c| c.look.yaw), 0.0);
    }

    #[test]
    fn touches_on_another_window_leave_the_view_alone() {
        let (mut app, id) = app();
        controls(&app, id, |c| c.mode = CameraMode::FirstPerson);
        let other = app.world_mut().spawn(Window::default()).id();
        touch(
            &mut app,
            other,
            1,
            TouchPhase::Started,
            Vec2::new(400.0, 200.0),
        );
        app.update();
        touch(
            &mut app,
            other,
            1,
            TouchPhase::Moved,
            Vec2::new(410.0, 200.0),
        );
        app.update();
        assert_eq!(controls(&app, id, |c| c.look.yaw), 0.0);
    }
}
