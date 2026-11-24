//! The camera maths of `packages/world/src/renderer/lookControls.ts`, and of
//! the studio editors' orbit cameras.

use std::f32::consts::PI;

use bevy::input::mouse::{MouseButton, MouseScrollUnit};
use bevy::math::{Vec2, Vec3};
use officexr_sim::physics::rules::BODY_Y;

use crate::{CameraMode, FixedCamera, OrbitCamera, RotateButton};

pub const PITCH_MIN: f32 = -1.3;
pub const PITCH_MAX: f32 = 1.3;
pub const MOUSE_SENSITIVITY: f32 = 0.0025;

pub const ZOOM_MIN: f32 = 0.4;
pub const ZOOM_MAX: f32 = 3.0;

const FIXED_ELEVATION_MIN: f32 = 10.0 * PI / 180.0;
const FIXED_ELEVATION_MAX: f32 = 85.0 * PI / 180.0;
const FOV_MIN_DEG: f32 = 30.0;
const FOV_MAX_DEG: f32 = 100.0;

pub const EYE_HEIGHT: f32 = 1.6;
/// Used by the screen-fraction → distance formula.
pub const CHAR_HEIGHT_M: f32 = 1.8;

const WHEEL_LINE_PX: f32 = 33.0;
/// One 100px notch of a typical mouse wheel is a ~10% zoom step.
const WHEEL_ZOOM_PER_PX: f32 = 0.001;

/// Per-view state that never leaves the client. In fixed mode the offsets
/// orbit the camera around its reference point: a positive azimuth offset
/// turns the view right and a positive pitch offset raises the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewOffsets {
    pub fixed_azimuth_offset_rad: f32,
    pub fixed_pitch_offset_rad: f32,
    /// Wheel (and later pinch) zoom, 1 = no zoom.
    pub zoom: f32,
}

impl Default for ViewOffsets {
    fn default() -> Self {
        Self {
            fixed_azimuth_offset_rad: 0.0,
            fixed_pitch_offset_rad: 0.0,
            zoom: 1.0,
        }
    }
}

/// The view's look direction: forward is `(-sin yaw, -cos yaw)`, positive
/// yaw turns counter-clockwise from above, positive pitch looks up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
    pub offsets: ViewOffsets,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -0.25,
            offsets: ViewOffsets::default(),
        }
    }
}

/// Modes whose yaw and pitch the mouse drives.
pub fn is_yaw_pitch_mode(mode: CameraMode) -> bool {
    matches!(mode, CameraMode::FirstPerson | CameraMode::ThirdPerson)
}

/// Modes the wheel zooms.
pub fn is_look_mode(mode: CameraMode) -> bool {
    is_yaw_pitch_mode(mode) || mode == CameraMode::Fixed
}

/// Turns the view by `d_yaw`/`d_pitch` radians (positive = counter-clockwise
/// yaw, look up). Fixed mode's elevation offset is clamped by [`fixed_leash`],
/// which knows the base pitch.
pub fn apply_look_delta(mode: CameraMode, d_yaw: f32, d_pitch: f32, look: &mut Look) {
    if is_yaw_pitch_mode(mode) {
        look.yaw += d_yaw;
        look.pitch = (look.pitch + d_pitch).clamp(PITCH_MIN, PITCH_MAX);
    } else if mode == CameraMode::Fixed {
        look.offsets.fixed_azimuth_offset_rad -= d_yaw;
        look.offsets.fixed_pitch_offset_rad -= d_pitch;
    }
}

pub fn apply_zoom(factor: f32, offsets: &mut ViewOffsets) {
    offsets.zoom = (offsets.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
}

/// As `lookControls.ts`: a drag of `delta` px as `(d_yaw, d_pitch)` radians for
/// [`apply_look_delta`]; `sign` is -1, or 1 with inverted look.
pub fn drag_look_delta(delta: Vec2, sensitivity: f32, sign: f32) -> Vec2 {
    delta * sign * sensitivity
}

/// A wheel delta in DOM units: lines, or CSS px. Positive scrolls down.
pub fn dom_wheel_delta(y: f32, unit: MouseScrollUnit, scale_factor: f32) -> f32 {
    match unit {
        MouseScrollUnit::Line => -y,
        MouseScrollUnit::Pixel => -y / scale_factor,
    }
}

/// Zoom factor for a DOM wheel delta: scrolling up (negative) zooms in, by a
/// step proportional to the distance scrolled.
// ponytail: a trackpad pinch (ctrl+wheel on the web) zooms at wheel speed, not 10x faster, as Bevy's wheel event has no ctrl flag.
pub fn wheel_zoom_factor(delta_y: f32, unit: MouseScrollUnit) -> f32 {
    let px = match unit {
        MouseScrollUnit::Line => WHEEL_LINE_PX,
        MouseScrollUnit::Pixel => 1.0,
    };
    (-delta_y * px * WHEEL_ZOOM_PER_PX).exp()
}

/// Field of view for a zoom, interpolated geometrically so every zoom in
/// [`ZOOM_MIN`, `ZOOM_MAX`] maps to a distinct fov: `base_fov_deg` at zoom 1,
/// 100° at `ZOOM_MIN` and 30° at `ZOOM_MAX`.
pub fn zoomed_fov(base_fov_deg: f32, zoom: f32) -> f32 {
    let (limit_deg, zoom_limit) = if zoom < 1.0 {
        (FOV_MAX_DEG, ZOOM_MIN)
    } else {
        (FOV_MIN_DEG, ZOOM_MAX)
    };
    base_fov_deg * (limit_deg / base_fov_deg).powf(zoom.ln() / zoom_limit.ln())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leash {
    /// The pitch offset in effect, to write back so a drag past the limit doesn't wind up.
    pub pitch_offset_rad: f32,
    pub pitch_rad: f32,
    pub height: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub max_lateral: f32,
}

/// Fixed-camera pose limits. At the base pitch the camera sits `height` above
/// the reference and keeps its XZ distance within [min, max] (and ±max_lateral
/// sideways), sized so the character fills between `max_on_screen_frac` and
/// `min_on_screen_frac` of the screen. The pitch offset then orbits that pose
/// about the reference within 10°–85° elevation (widened to include the base),
/// and zoom divides the whole camera→reference offset.
pub fn fixed_leash(base_pitch_rad: f32, fixed: &FixedCamera, offsets: &ViewOffsets) -> Leash {
    let base_elevation = -base_pitch_rad;
    let pitch_offset_rad = if base_elevation > 0.0 {
        offsets.fixed_pitch_offset_rad.clamp(
            FIXED_ELEVATION_MIN.min(base_elevation) - base_elevation,
            FIXED_ELEVATION_MAX.max(base_elevation) - base_elevation,
        )
    } else {
        0.0
    };
    let elevation = base_elevation + pitch_offset_rad;
    let (height_scale, distance_scale) = if pitch_offset_rad == 0.0 {
        (1.0, 1.0)
    } else {
        (
            elevation.sin() / base_elevation.sin(),
            elevation.cos() / base_elevation.cos(),
        )
    };

    let tan_half_fov = (fixed.fov.to_radians() / 2.0).tan();
    let height_offset = (fixed.height - CHAR_HEIGHT_M / 2.0).max(0.0);
    let d_near_3d = CHAR_HEIGHT_M / (2.0 * fixed.max_on_screen_frac.max(0.005) * tan_half_fov);
    let d_far_3d = CHAR_HEIGHT_M / (2.0 * fixed.min_on_screen_frac.max(0.001) * tan_half_fov);

    // When the requested screen fraction is impossible at this height, fall
    // back to a distance that keeps the character along the look direction.
    let tan_pitch = base_pitch_rad.abs().tan();
    let pitch_aligned_xz = if tan_pitch > 0.01 {
        height_offset / tan_pitch
    } else {
        height_offset
    };
    let xz = |d_3d: f32, fallback: f32| {
        let d = if d_3d > height_offset {
            (d_3d * d_3d - height_offset * height_offset).sqrt()
        } else {
            pitch_aligned_xz * fallback
        };
        d * distance_scale
    };

    let direct_xz = fixed.distance_m.map(|d| d * distance_scale);
    let min_distance = direct_xz.unwrap_or_else(|| xz(d_near_3d, 0.6)).max(0.1);
    let max_distance = match direct_xz {
        Some(_) => min_distance,
        None => (min_distance + 0.5).max(xz(d_far_3d, 1.6)),
    };

    let zoom = offsets.zoom;
    let max_leash = max_distance / zoom;
    Leash {
        pitch_offset_rad,
        pitch_rad: base_pitch_rad - pitch_offset_rad,
        height: fixed.height * height_scale / zoom,
        min_distance: min_distance / zoom,
        max_distance: max_leash,
        max_lateral: match direct_xz {
            Some(_) => 0.0,
            None => fixed.lateral_frac * max_leash * tan_half_fov,
        },
    }
}

/// Height of the character's chest above its body root.
pub fn chest_rise(char_radius: f32) -> f32 {
    BODY_Y as f32 - char_radius + CHAR_HEIGHT_M / 2.0
}

/// `lookControls.ts`' `centredFixedPose`: a fixed camera that keeps the
/// character's chest on its look axis, as the fixed config to frame the
/// chest with (`height` above it, at the XZ `distance_m` where the look ray
/// at `pitch_deg` meets it) and the chest's rise above the body root. Zoom
/// and elevation offsets scale the pose about the chest ([`fixed_leash`]),
/// so the character stays centred under them too.
pub fn centred_fixed(fixed: &FixedCamera, char_radius: f32) -> (FixedCamera, f32) {
    let rise = chest_rise(char_radius);
    let above_chest = fixed.height - rise;
    let centred = FixedCamera {
        height: above_chest,
        distance_m: Some(above_chest / fixed.pitch_deg.abs().to_radians().tan()),
        ..fixed.clone()
    };
    (centred, rise)
}

/// Yaw of the fixed camera for camera-relative movement. The camera sits at
/// `(sin az, -cos az)` from the reference, so turning the view right (a
/// positive azimuth offset) lowers the yaw.
pub fn fixed_movement_yaw(
    azimuth_deg: f32,
    azimuth_offset_rad: f32,
    movement_yaw_offset_deg: f32,
) -> f32 {
    PI - azimuth_deg.to_radians() - azimuth_offset_rad + movement_yaw_offset_deg.to_radians()
}

/// Yaw of a look direction, ignoring its pitch.
pub fn yaw_from_direction(dir: Vec2) -> f32 {
    (-dir.x).atan2(-dir.y)
}

/// Where a character's eyes are, from its body root.
pub fn eye_position(pos: Vec3, char_radius: f32) -> Vec3 {
    pos + Vec3::Y * (BODY_Y as f32 - char_radius + EYE_HEIGHT)
}

/// Camera position `r` from `centre`, behind the look direction of `yaw`, for
/// a camera aimed at `centre`. A look-down (negative) pitch raises it.
pub fn orbit_position(centre: Vec3, yaw: f32, pitch: f32, r: f32) -> Vec3 {
    let cos_p = pitch.cos();
    centre + Vec3::new(yaw.sin() * cos_p, (-pitch).sin(), yaw.cos() * cos_p) * r
}

/// The unit look vector of `yaw` and `pitch`.
pub fn forward(yaw: f32, pitch: f32) -> Vec3 {
    let cos_p = pitch.cos();
    Vec3::new(-yaw.sin() * cos_p, pitch.sin(), -yaw.cos() * cos_p)
}

/// Drag px → radians, for the orbit camera and free-fly's turn.
pub const DRAG_ROTATE: f32 = 0.005;
/// Drag px → metres of pan, per metre of orbit distance.
const ORBIT_PAN: f32 = 0.0015;
/// Just short of the poles, where the look-at up vector degenerates.
pub const ORBIT_POLE: f32 = std::f32::consts::FRAC_PI_2 - 0.05;
/// drei's zoom scale for one dolly step.
const DOLLY_STEP: f32 = 0.95;
/// Metres per second the move keys carry the orbit target, per metre of distance.
const ORBIT_MOVE: f32 = 1.2;

/// What a mouse drag does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drag {
    /// Free-fly's turn in place.
    Turn,
    Rotate,
    Pan,
    /// drei's middle-drag zoom.
    Dolly,
}

/// Modifier keys held as a drag starts.
#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    /// Ctrl, or cmd on macOS.
    pub ctrl: bool,
}

/// The drag a press of `button` starts in `mode`, if any.
pub fn drag_for(
    mode: CameraMode,
    button: MouseButton,
    held: Modifiers,
    orbit: &OrbitCamera,
) -> Option<Drag> {
    let pan = orbit.pan.then_some(Drag::Pan);
    match (mode, orbit.rotate_button, button) {
        (CameraMode::FreeFly, _, MouseButton::Left) => Some(Drag::Turn),
        // The scene editor's; with panning off, as in the character editor, shift is ignored.
        (CameraMode::Orbit, RotateButton::Right, MouseButton::Right) => Some(if held.shift {
            pan.unwrap_or(Drag::Rotate)
        } else {
            Drag::Rotate
        }),
        (CameraMode::Orbit, RotateButton::Right, MouseButton::Middle) => pan,
        // drei `OrbitControls`': a modifier swaps the left and right buttons' rotate and pan.
        (CameraMode::Orbit, RotateButton::Left, MouseButton::Left | MouseButton::Right) => {
            let swapped = held.shift || held.ctrl;
            if (button == MouseButton::Left) != swapped {
                Some(Drag::Rotate)
            } else {
                pan
            }
        }
        (CameraMode::Orbit, RotateButton::Left, MouseButton::Middle) => Some(Drag::Dolly),
        _ => None,
    }
}

/// An orbit camera, at `distance` from `target` toward `azimuth` (its look
/// yaw) and `elevation` (positive is above the target).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub target: Vec3,
    pub azimuth: f32,
    pub elevation: f32,
    pub distance: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            target: Vec3::Y,
            azimuth: -0.55,
            elevation: 0.45,
            distance: 16.0,
        }
    }
}

impl Orbit {
    pub fn position(&self) -> Vec3 {
        orbit_position(self.target, self.azimuth, -self.elevation, self.distance)
    }

    /// Within the config's limits, and short of the poles whatever they say.
    /// `max`/`min` rather than `clamp`, which panics on a config whose
    /// minimum passes its maximum.
    pub fn clamped(mut self, config: &OrbitCamera) -> Self {
        self.elevation = self
            .elevation
            .max(config.min_elevation_deg.to_radians().max(-ORBIT_POLE))
            .min(config.max_elevation_deg.to_radians().min(ORBIT_POLE));
        self.distance = self
            .distance
            .max(config.min_distance)
            .min(config.max_distance);
        self
    }

    /// A drag of `delta` CSS px swings the camera around the target.
    // ponytail: drei turns 2π per canvas height and zooms 5% a wheel notch; every orbit here uses the scene editor's per-px rates.
    pub fn rotate(&mut self, delta: Vec2, config: &OrbitCamera) {
        self.azimuth -= delta.x * DRAG_ROTATE;
        self.elevation -= delta.y * DRAG_ROTATE;
        *self = self.clamped(config);
    }

    /// A drag of `delta` CSS px slides the target across the view, faster
    /// the further out the camera is.
    pub fn pan(&mut self, delta: Vec2) {
        let speed = self.distance * ORBIT_PAN;
        let back = orbit_position(Vec3::ZERO, self.azimuth, -self.elevation, 1.0);
        let right = Vec3::Y.cross(back).normalize();
        let up = back.cross(right);
        self.target += (right * -delta.x + up * delta.y) * speed;
    }

    /// Divides the distance by a [`wheel_zoom_factor`].
    pub fn zoom(&mut self, factor: f32, config: &OrbitCamera) {
        self.distance /= factor;
        *self = self.clamped(config);
    }

    /// drei's middle-drag: 5% further out or in for a drag down or up.
    // ponytail: one step a frame where drei steps per pointermove; browsers send about one pointermove a frame.
    pub fn dolly(&mut self, delta_y: f32, config: &OrbitCamera) {
        if delta_y != 0.0 {
            self.zoom(DOLLY_STEP.powf(delta_y.signum()), config);
        }
    }

    /// Carries the target along held move-key `axes` (`x` right, `y` up, `z`
    /// forward) for `dt` seconds, level with the ground whatever the elevation.
    pub fn fly(&mut self, axes: Vec3, dt: f32) {
        let (sin, cos) = self.azimuth.sin_cos();
        let forward = Vec3::new(-sin, 0.0, -cos);
        let right = Vec3::new(cos, 0.0, -sin);
        self.target += (forward * axes.z + right * axes.x + Vec3::Y * axes.y)
            * self.distance
            * ORBIT_MOVE
            * dt;
    }

    /// Aims at the box's centre from far enough out that its bounding sphere
    /// fits the vertical field of view, keeping the angles.
    // ponytail: a view narrower than it is tall can still crop the box at the sides; the rig doesn't know the aspect.
    pub fn frame(&mut self, min: Vec3, max: Vec3, config: &OrbitCamera) {
        self.target = (min + max) / 2.0;
        self.distance = min.distance(max) / 2.0 / (config.fov().to_radians() / 2.0).sin();
    }

    /// The orbit that puts the camera at `eye`, looking at `target`.
    pub fn from_eye(target: Vec3, eye: Vec3) -> Self {
        let back = eye - target;
        Self {
            target,
            azimuth: back.x.atan2(back.z),
            elevation: back.y.atan2(Vec2::new(back.x, back.z).length()),
            distance: back.length(),
        }
    }

    /// `t` of the way to `goal`. Azimuth winds up rather than wrapping, so a
    /// plain lerp never takes the long way round.
    pub fn eased_toward(self, goal: Self, t: f32) -> Self {
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Self {
            target: self.target.lerp(goal.target, t),
            azimuth: lerp(self.azimuth, goal.azimuth),
            elevation: lerp(self.elevation, goal.elevation),
            distance: lerp(self.distance, goal.distance),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < EPS, "{a} != {b}");
    }

    fn deg(d: f32) -> f32 {
        d.to_radians()
    }

    fn offsets(pitch: f32, zoom: f32) -> ViewOffsets {
        ViewOffsets {
            fixed_pitch_offset_rad: pitch,
            zoom,
            ..ViewOffsets::default()
        }
    }

    #[test]
    fn look_delta_drives_yaw_and_pitch_in_first_and_third_person() {
        for mode in [CameraMode::FirstPerson, CameraMode::ThirdPerson] {
            let mut look = Look::default();
            apply_look_delta(mode, 0.1, 0.2, &mut look);
            close(look.yaw, 0.1);
            close(look.pitch, -0.05);
            assert_eq!(look.offsets, ViewOffsets::default());
        }
    }

    #[test]
    fn look_delta_clamps_pitch() {
        let mut look = Look::default();
        apply_look_delta(CameraMode::FirstPerson, 0.0, 100.0, &mut look);
        assert_eq!(look.pitch, PITCH_MAX);
        apply_look_delta(CameraMode::FirstPerson, 0.0, -100.0, &mut look);
        assert_eq!(look.pitch, PITCH_MIN);
    }

    #[test]
    fn look_delta_orbits_the_fixed_camera_and_leaves_yaw_and_pitch_alone() {
        let mut look = Look::default();
        apply_look_delta(CameraMode::Fixed, 0.1, 0.2, &mut look);
        assert_eq!((look.yaw, look.pitch), (0.0, -0.25));
        close(look.offsets.fixed_azimuth_offset_rad, -0.1);
        close(look.offsets.fixed_pitch_offset_rad, -0.2);
    }

    #[test]
    fn look_delta_ignores_top_down_free_fly_and_orbit() {
        for mode in [CameraMode::TopDown, CameraMode::FreeFly, CameraMode::Orbit] {
            let mut look = Look::default();
            apply_look_delta(mode, 1.0, 1.0, &mut look);
            assert_eq!(look, Look::default());
        }
    }

    #[test]
    fn zoom_multiplies_and_clamps() {
        let mut o = ViewOffsets::default();
        apply_zoom(1.5, &mut o);
        assert_eq!(o.zoom, 1.5);
        apply_zoom(100.0, &mut o);
        assert_eq!(o.zoom, ZOOM_MAX);
        apply_zoom(0.001, &mut o);
        assert_eq!(o.zoom, ZOOM_MIN);
    }

    #[test]
    fn wheel_zooms_in_scrolling_up_by_about_ten_percent_a_notch() {
        let px = MouseScrollUnit::Pixel;
        assert!(wheel_zoom_factor(-100.0, px) > 1.0);
        assert!(wheel_zoom_factor(100.0, px) < 1.0);
        assert_eq!(wheel_zoom_factor(0.0, px), 1.0);
        assert!((wheel_zoom_factor(-100.0, px) - 1.1).abs() < 0.05);
        close(
            wheel_zoom_factor(-100.0, px) * wheel_zoom_factor(100.0, px),
            1.0,
        );
    }

    #[test]
    fn wheel_zoom_is_proportional_and_normalises_lines() {
        let px = MouseScrollUnit::Pixel;
        let zoom = (0..100).fold(1.0, |z, _| z * wheel_zoom_factor(-1.0, px));
        close(zoom, wheel_zoom_factor(-100.0, px));
        let line = wheel_zoom_factor(-3.0, MouseScrollUnit::Line);
        assert!((line - wheel_zoom_factor(-100.0, px)).abs() < 0.05);
        assert_eq!(dom_wheel_delta(1.0, MouseScrollUnit::Line, 2.0), -1.0);
        assert_eq!(dom_wheel_delta(200.0, px, 2.0), -100.0);
    }

    #[test]
    fn wheel_zoom_stays_inside_the_clamp() {
        let mut o = ViewOffsets::default();
        for _ in 0..200 {
            apply_zoom(wheel_zoom_factor(-100.0, MouseScrollUnit::Pixel), &mut o);
        }
        assert_eq!(o.zoom, ZOOM_MAX);
        for _ in 0..400 {
            apply_zoom(wheel_zoom_factor(100.0, MouseScrollUnit::Pixel), &mut o);
        }
        assert_eq!(o.zoom, ZOOM_MIN);
    }

    #[test]
    fn zoomed_fov_maps_the_zoom_range_onto_the_fov_bounds() {
        assert_eq!(zoomed_fov(75.0, 1.0), 75.0);
        close(zoomed_fov(75.0, ZOOM_MIN), 100.0);
        close(zoomed_fov(75.0, ZOOM_MAX), 30.0);
        let mut prev = f32::INFINITY;
        let mut z = ZOOM_MIN;
        while z <= ZOOM_MAX {
            let fov = zoomed_fov(75.0, z);
            assert!(fov < prev);
            prev = fov;
            z += 0.1;
        }
    }

    fn defaults() -> FixedCamera {
        FixedCamera {
            height: 25.0,
            max_on_screen_frac: 0.25,
            min_on_screen_frac: 0.08,
            lateral_frac: 0.4,
            fov: 50.0,
            ..FixedCamera::default()
        }
    }

    #[test]
    fn leash_reproduces_the_unrotated_screen_fraction_leash() {
        let leash = fixed_leash(deg(-35.0), &defaults(), &ViewOffsets::default());
        let tan_half_fov = deg(25.0).tan();
        let h_off = 25.0 - CHAR_HEIGHT_M / 2.0;
        let near_3d = CHAR_HEIGHT_M / (2.0 * 0.25 * tan_half_fov);
        let far_3d = CHAR_HEIGHT_M / (2.0 * 0.08 * tan_half_fov);
        let min = h_off / deg(35.0).tan() * 0.6;
        let max = (far_3d * far_3d - h_off * h_off).sqrt();
        assert_eq!(leash.height, 25.0);
        assert_eq!(leash.pitch_rad, deg(-35.0));
        assert!(near_3d < h_off && far_3d > h_off);
        close(leash.min_distance, min);
        close(leash.max_distance, (min + 0.5).max(max));
        close(leash.max_lateral, 0.4 * leash.max_distance * tan_half_fov);
    }

    #[test]
    fn leash_keeps_the_avatar_within_the_fov_across_elevation_and_zoom() {
        for elevation in [deg(10.0), deg(35.0), deg(85.0)] {
            for zoom in [ZOOM_MIN, 1.0, ZOOM_MAX] {
                let o = offsets(elevation - deg(35.0), zoom);
                let leash = fixed_leash(deg(-35.0), &defaults(), &o);
                close(leash.pitch_rad, -elevation);
                for depth in [leash.min_distance, leash.max_distance] {
                    for y in [0.0, CHAR_HEIGHT_M / 2.0, CHAR_HEIGHT_M] {
                        let below = (leash.height - y).atan2(depth);
                        assert!((below - elevation).abs() < deg(25.0));
                    }
                }
            }
        }
    }

    #[test]
    fn leash_rotates_a_pinned_distance_with_the_elevation() {
        let base = deg(8.0);
        let pinned = FixedCamera {
            height: 4.0 * base.tan(),
            distance_m: Some(4.0),
            ..defaults()
        };
        let flat = fixed_leash(-base, &pinned, &ViewOffsets::default());
        assert_eq!((flat.min_distance, flat.max_distance), (4.0, 4.0));
        assert_eq!(flat.max_lateral, 0.0);
        let steep = fixed_leash(-base, &pinned, &offsets(deg(90.0), 1.0));
        close(steep.pitch_rad, deg(-85.0));
        close(steep.min_distance, 4.0 * deg(85.0).cos() / base.cos());
        close(steep.height.atan2(steep.min_distance), deg(85.0));
    }

    #[test]
    fn leash_clamps_the_elevation_and_reports_the_offset_in_effect() {
        let top = fixed_leash(deg(-35.0), &defaults(), &offsets(deg(90.0), 1.0));
        close(top.pitch_rad, deg(-85.0));
        close(top.pitch_offset_rad, deg(50.0));
        let bottom = fixed_leash(deg(-35.0), &defaults(), &offsets(deg(-90.0), 1.0));
        close(bottom.pitch_rad, deg(-10.0));
        close(bottom.pitch_offset_rad, deg(-25.0));
    }

    #[test]
    fn leash_keeps_a_base_elevation_outside_the_clamp_unchanged() {
        let shallow = fixed_leash(deg(-8.0), &defaults(), &offsets(deg(-5.0), 1.0));
        assert_eq!(shallow.pitch_rad, deg(-8.0));
        assert_eq!(shallow.pitch_offset_rad, 0.0);
    }

    #[test]
    fn leash_does_not_orbit_a_camera_that_does_not_look_down() {
        let level = fixed_leash(0.0, &defaults(), &offsets(deg(30.0), 1.0));
        assert_eq!(level.pitch_offset_rad, 0.0);
        assert_eq!(level.height, 25.0);
    }

    #[test]
    fn fixed_movement_yaw_follows_the_azimuth_and_its_offsets() {
        close(fixed_movement_yaw(202.5, 0.0, 0.0), PI - deg(202.5));
        close(
            fixed_movement_yaw(202.5, 0.0, 10.0),
            PI - deg(202.5) + deg(10.0),
        );
        close(fixed_movement_yaw(202.5, 0.3, 0.0), PI - deg(202.5) - 0.3);
        // A right-hand drag turns movement as first-person look would.
        let mut look = Look::default();
        apply_look_delta(CameraMode::Fixed, -0.5, 0.0, &mut look);
        let yaw = fixed_movement_yaw(0.0, look.offsets.fixed_azimuth_offset_rad, 0.0);
        close(yaw, PI - 0.5);
    }

    #[test]
    fn yaw_from_direction_inverts_the_yaw_convention_whatever_the_pitch() {
        for yaw in [0.0, 0.7, -2.4, PI / 2.0] {
            for pitch in [0.0, -0.6, 1.1] {
                let dir = forward(yaw, pitch);
                close(yaw_from_direction(Vec2::new(dir.x, dir.z)), yaw);
            }
        }
    }

    #[test]
    fn eye_and_orbit_positions() {
        let root = Vec3::new(2.0, 1.0, -3.0);
        for (char_radius, eye_y) in [(0.4, 3.1), (0.25, 3.25)] {
            let eye = eye_position(root, char_radius);
            close(eye.y, eye_y);
            assert_eq!((eye.x, eye.z), (root.x, root.z));
            let level = orbit_position(eye, 0.7, 0.0, 6.0);
            close(level.y, eye.y);
            close(Vec2::new(level.x - eye.x, level.z - eye.z).length(), 6.0);
            let above = orbit_position(eye, 0.7, -0.25, 6.0);
            assert!(above.y > eye.y);
            close(above.distance(eye), 6.0);
        }
        let behind = orbit_position(Vec3::ZERO, 0.0, 0.0, 5.0);
        close(behind.x, 0.0);
        close(behind.z, 5.0);
        close(
            orbit_position(Vec3::ZERO, 0.0, 0.4, 5.0).y,
            -5.0 * 0.4f32.sin(),
        );
    }

    fn close3(a: Vec3, b: Vec3) {
        assert!(a.distance(b) < 1e-3, "{a} != {b}");
    }

    /// `SceneEditorCanvas`'s `OrbitCamera`'s `useFrame` placement.
    fn scene_editor_position(o: &Orbit) -> Vec3 {
        let (az, el, d) = (o.azimuth, o.elevation, o.distance);
        o.target
            + Vec3::new(
                d * el.cos() * az.sin(),
                d * el.sin(),
                d * el.cos() * az.cos(),
            )
    }

    #[test]
    fn orbit_sits_where_the_scene_editor_puts_its_camera() {
        let orbit = Orbit::default();
        assert_eq!(
            (orbit.azimuth, orbit.elevation, orbit.distance),
            (-0.55, 0.45, 16.0)
        );
        close3(orbit.position(), scene_editor_position(&orbit));
        let swung = Orbit {
            target: Vec3::new(3.0, -2.0, 5.0),
            azimuth: 2.4,
            elevation: -0.9,
            distance: 7.5,
        };
        close3(swung.position(), scene_editor_position(&swung));
    }

    #[test]
    fn orbit_rotates_half_a_centiradian_per_px_short_of_the_poles() {
        let config = OrbitCamera::default();
        let mut orbit = Orbit::default();
        orbit.rotate(Vec2::new(100.0, 40.0), &config);
        close(orbit.azimuth, -1.05);
        close(orbit.elevation, 0.25);
        orbit.rotate(Vec2::new(0.0, -1000.0), &config);
        close(orbit.elevation, PI / 2.0 - 0.05);
        orbit.rotate(Vec2::new(0.0, 1000.0), &config);
        close(orbit.elevation, -PI / 2.0 + 0.05);

        // The character editor's limits.
        let character = OrbitCamera {
            min_elevation_deg: (-0.4f32).to_degrees(),
            ..OrbitCamera::default()
        };
        orbit.rotate(Vec2::ZERO, &character);
        close(orbit.elevation, -0.4);
    }

    #[test]
    fn orbit_pans_the_target_across_the_view_by_distance() {
        let mut orbit = Orbit::default();
        let back = (orbit.position() - orbit.target).normalize();
        orbit.pan(Vec2::new(100.0, 0.0));
        // 100 px × 16 m × 0.0015, to the left along the level right axis.
        let moved = orbit.target - Orbit::default().target;
        close(moved.length(), 2.4);
        close(moved.y, 0.0);
        close(moved.dot(back), 0.0);
        let right = Vec3::new(orbit.azimuth.cos(), 0.0, -orbit.azimuth.sin());
        close3(moved, right * -2.4);

        let mut orbit = Orbit::default();
        orbit.pan(Vec2::new(0.0, 100.0));
        let moved = orbit.target - Orbit::default().target;
        close(moved.length(), 2.4);
        close(moved.dot(back), 0.0);
        assert!(moved.y > 0.0, "a drag down raises the target");
    }

    #[test]
    fn orbit_zooms_by_the_wheel_within_its_distance_limits() {
        let config = OrbitCamera::default();
        let mut orbit = Orbit::default();
        // A 100 px wheel down is the scene editor's `exp(deltaY * 0.001)`.
        orbit.zoom(wheel_zoom_factor(100.0, MouseScrollUnit::Pixel), &config);
        close(orbit.distance, 16.0 * 0.1f32.exp());
        orbit.zoom(
            wheel_zoom_factor(-10_000.0, MouseScrollUnit::Pixel),
            &config,
        );
        close(orbit.distance, 2.0);
        orbit.zoom(wheel_zoom_factor(10_000.0, MouseScrollUnit::Pixel), &config);
        close(orbit.distance, 200.0);

        // `EditorCamera`'s limits.
        let preview = OrbitCamera {
            min_distance: 1.5,
            max_distance: 40.0,
            ..OrbitCamera::default()
        };
        orbit.zoom(1.0, &preview);
        close(orbit.distance, 40.0);
    }

    #[test]
    fn a_config_with_crossed_limits_does_not_panic() {
        let crossed = OrbitCamera {
            min_distance: 10.0,
            max_distance: 5.0,
            min_elevation_deg: 30.0,
            max_elevation_deg: -30.0,
            ..OrbitCamera::default()
        };
        let orbit = Orbit::default().clamped(&crossed);
        assert!(orbit.distance.is_finite() && orbit.elevation.is_finite());
    }

    #[test]
    fn frame_bounds_fits_the_box_in_the_fov_and_keeps_the_angles() {
        let config = OrbitCamera::default();
        let mut orbit = Orbit::default();
        let (min, max) = (Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0));
        orbit.frame(min, max, &config);
        close3(orbit.target, Vec3::Y);
        assert_eq!((orbit.azimuth, orbit.elevation), (-0.55, 0.45));
        // The bounding sphere (radius √3) touches the edge of the 45° view.
        close((3f32.sqrt() / orbit.distance).asin(), 22.5f32.to_radians());

        orbit.frame(Vec3::splat(-500.0), Vec3::splat(500.0), &config);
        close(orbit.clamped(&config).distance, 200.0);
        orbit.frame(Vec3::ONE, Vec3::ONE, &config);
        close(orbit.clamped(&config).distance, 2.0);

        let zero_fov = OrbitCamera {
            fov: 0.0,
            ..OrbitCamera::default()
        };
        orbit.frame(min, max, &zero_fov);
        assert!(orbit.distance.is_finite());
    }

    #[test]
    fn orbit_drag_buttons() {
        let scene = OrbitCamera::default();
        let character = OrbitCamera {
            pan: false,
            ..OrbitCamera::default()
        };
        let drei = OrbitCamera {
            rotate_button: RotateButton::Left,
            ..OrbitCamera::default()
        };
        let preview = OrbitCamera {
            pan: false,
            ..drei.clone()
        };
        use MouseButton::{Left, Middle, Right};
        let none = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..none
        };
        let ctrl = Modifiers { ctrl: true, ..none };
        let cases = [
            // The scene editor: right rotates, middle or shift+right pans, left is the tools'.
            (&scene, Right, none, Some(Drag::Rotate)),
            (&scene, Right, shift, Some(Drag::Pan)),
            (&scene, Middle, none, Some(Drag::Pan)),
            (&scene, Left, none, None),
            (&scene, Left, shift, None),
            // The character editor: right rotates, shift or not.
            (&character, Right, shift, Some(Drag::Rotate)),
            (&character, Middle, none, None),
            // drei: left rotates, right pans, a modifier swaps them, middle dollies.
            (&drei, Left, none, Some(Drag::Rotate)),
            (&drei, Left, shift, Some(Drag::Pan)),
            (&drei, Left, ctrl, Some(Drag::Pan)),
            (&drei, Right, none, Some(Drag::Pan)),
            (&drei, Right, ctrl, Some(Drag::Rotate)),
            (&drei, Middle, none, Some(Drag::Dolly)),
            (&preview, Left, none, Some(Drag::Rotate)),
            (&preview, Right, none, None),
            (&preview, Right, shift, Some(Drag::Rotate)),
            (&preview, Left, shift, None),
            (&preview, Middle, none, Some(Drag::Dolly)),
        ];
        for (config, button, held, expected) in cases {
            assert_eq!(
                drag_for(CameraMode::Orbit, button, held, config),
                expected,
                "{button:?} {held:?} {:?} pan {}",
                config.rotate_button,
                config.pan,
            );
        }
        assert_eq!(
            drag_for(CameraMode::FreeFly, Left, none, &scene),
            Some(Drag::Turn)
        );
        assert_eq!(drag_for(CameraMode::FreeFly, Right, none, &scene), None);
        assert_eq!(drag_for(CameraMode::Fixed, Left, none, &scene), None);
    }

    #[test]
    fn a_middle_drag_dollies_five_percent_a_frame_like_drei() {
        let config = OrbitCamera::default();
        let mut orbit = Orbit::default();
        orbit.dolly(3.0, &config);
        close(orbit.distance, 16.0 / 0.95);
        orbit.dolly(0.0, &config);
        close(orbit.distance, 16.0 / 0.95);
        orbit.dolly(-40.0, &config);
        close(orbit.distance, 16.0);
    }

    #[test]
    fn elevation_stays_short_of_the_poles_whatever_the_limits() {
        let past_the_poles = OrbitCamera {
            min_elevation_deg: -120.0,
            max_elevation_deg: 95.0,
            ..OrbitCamera::default()
        };
        let mut orbit = Orbit::default();
        orbit.rotate(Vec2::new(0.0, -10_000.0), &past_the_poles);
        close(orbit.elevation, ORBIT_POLE);
        orbit.rotate(Vec2::new(0.0, 10_000.0), &past_the_poles);
        close(orbit.elevation, -ORBIT_POLE);
    }

    /// The map editor's `FlyCamera`: WASD moves the target level with the
    /// ground and Q/E up and down, at distance × 1.2 m/s, around an orbit
    /// that starts at azimuth -0.55, elevation 0.7 and 60 m.
    #[test]
    fn orbit_move_keys_match_the_map_editors_fly_camera() {
        let start = Orbit {
            target: Vec3::ZERO,
            azimuth: -0.55,
            elevation: 0.7,
            distance: 60.0,
        };
        // `FlyCamera`'s forward: target − camera, flattened.
        let mut ts_forward = start.target - scene_editor_position(&start);
        ts_forward.y = 0.0;
        let ts_forward = ts_forward.normalize();
        let ts_right = ts_forward.cross(Vec3::Y).normalize();

        let mut orbit = start;
        orbit.fly(Vec3::Z, 0.5);
        close3(orbit.target, ts_forward * 36.0);
        let mut orbit = start;
        orbit.fly(Vec3::new(-1.0, 1.0, 0.0), 0.5);
        close3(orbit.target, ts_right * -36.0 + Vec3::Y * 36.0);
    }

    #[test]
    fn an_eye_and_target_round_trip_through_the_orbit() {
        // `ObjectPreviewCanvas`'s `EditorCamera`.
        let (target, eye) = (Vec3::new(0.0, 0.5, 0.0), Vec3::new(2.5, 2.0, 2.5));
        let orbit = Orbit::from_eye(target, eye);
        close3(orbit.position(), eye);
        close(orbit.azimuth, PI / 4.0);
        close(orbit.distance, 14.75f32.sqrt());
        for eye in [Vec3::new(-3.0, -1.0, 0.5), Vec3::new(0.0, 4.0, -7.0)] {
            close3(Orbit::from_eye(target, eye).position(), eye);
        }
        let degenerate = Orbit::from_eye(target, target);
        assert!(degenerate.azimuth.is_finite() && degenerate.elevation.is_finite());
    }
}
