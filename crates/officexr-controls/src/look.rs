//! The camera maths of `packages/world/src/renderer/lookControls.ts`.

use std::f32::consts::PI;

use bevy::input::mouse::MouseScrollUnit;
use bevy::math::{Vec2, Vec3};
use officexr_sim::physics::rules::BODY_Y;

use crate::{CameraMode, FixedCamera};

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
    fn look_delta_ignores_top_down_and_free_fly() {
        for mode in [CameraMode::TopDown, CameraMode::FreeFly] {
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
}
