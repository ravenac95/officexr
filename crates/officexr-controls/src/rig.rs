//! Each camera mode's pose, as `packages/world/src/renderer/CameraRig.tsx`'s frame callback.

use std::f32::consts::PI;

use bevy::math::{Vec2, Vec3};
use officexr_render::SceneCamera;

use crate::CameraMode;
use crate::config::CameraConfig;
use crate::look::{
    self, CHAR_HEIGHT_M, Look, PITCH_MAX, PITCH_MIN, eye_position, fixed_leash, orbit_position,
    zoomed_fov,
};

const GAMEPLAY_FOV: f32 = 75.0;
const THIRD_PERSON_RADIUS: f32 = 6.0;
const NEAR: f32 = 0.1;
const FAR: f32 = 2000.0;
/// The conversation blend's damping time constant, in seconds.
const CONVERSATION_TAU: f32 = 0.35;

const TOP_DOWN_HEIGHT: f32 = 60.0;
const TOP_DOWN_FOV: f32 = 45.0;

const FREE_FLY_FOV: f32 = 60.0;
/// Metres per second; shift multiplies.
const FREE_FLY_SPEED: f32 = 12.0;
const FREE_FLY_SHIFT: f32 = 3.0;
/// Drag px → radians.
const FREE_FLY_ROTATE: f32 = 0.005;
/// World units per wheel notch.
const FREE_FLY_DOLLY: f32 = 1.5;

/// The editor camera that flies free of the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreeFly {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for FreeFly {
    fn default() -> Self {
        Self {
            pos: Vec3::new(0.0, 25.0, 25.0),
            yaw: 0.0,
            pitch: -0.7,
        }
    }
}

/// Held free-fly keys as axes: `x` right, `y` up, `z` forward, each -1..1.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlyKeys {
    pub axes: Vec3,
    pub fast: bool,
}

impl FreeFly {
    pub fn fly(&mut self, keys: FlyKeys, dt: f32) {
        let speed = FREE_FLY_SPEED * if keys.fast { FREE_FLY_SHIFT } else { 1.0 };
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let forward = look::forward(self.yaw, self.pitch);
        self.pos +=
            (forward * keys.axes.z + right * keys.axes.x + Vec3::Y * keys.axes.y) * speed * dt;
    }

    /// A drag of `delta` CSS px turns the view in place.
    // ponytail: no orbit around the object under the cursor, as the three.js free-fly drag does; that needs picking.
    pub fn drag(&mut self, delta: Vec2) {
        self.yaw -= delta.x * FREE_FLY_ROTATE;
        self.pitch = (self.pitch - delta.y * FREE_FLY_ROTATE).clamp(PITCH_MIN, PITCH_MAX);
    }

    /// Dollies along the view for a DOM wheel delta (positive = out).
    pub fn dolly(&mut self, delta_y: f32) {
        let step = -delta_y.signum() * FREE_FLY_DOLLY * (delta_y.abs() / 50.0).clamp(1.0, 8.0);
        self.pos += look::forward(self.yaw, self.pitch) * step;
    }
}

/// Fixed mode's camera, which only moves when the reference leaves its leash.
#[derive(Clone, Copy, Debug)]
struct FixedPose {
    position: Vec3,
    applied_azimuth_offset: f32,
}

/// The camera state that carries across frames.
#[derive(Clone, Debug, Default)]
pub struct Rig {
    /// 0..1, damped toward 1 while there is a conversation focus.
    conversation_mix: f32,
    /// Kept after the focus clears, to blend back from.
    last_focus: Vec3,
    /// `None` until fixed mode places it, and again whenever another mode is active.
    fixed: Option<FixedPose>,
    pub free_fly: FreeFly,
}

fn damp(from: f32, to: f32, lambda: f32, dt: f32) -> f32 {
    from + (to - from) * (1.0 - (-lambda * dt).exp())
}

fn pose(position: Vec3, look_at: Vec3, fov_deg: f32) -> SceneCamera {
    SceneCamera {
        position: position.into(),
        look_at: look_at.into(),
        fov_deg,
        near: NEAR,
        far: FAR,
    }
}

impl Rig {
    /// This frame's camera, `dt` seconds on. `target` is the followed
    /// player's body root; the modes that follow it need one.
    pub fn step(
        &mut self,
        mode: CameraMode,
        config: &CameraConfig,
        look: &mut Look,
        target: Option<Vec3>,
        focus: Option<Vec3>,
        dt: f32,
    ) -> Option<SceneCamera> {
        if let Some(focus) = focus {
            self.last_focus = focus;
        }
        let goal = if focus.is_some() { 1.0 } else { 0.0 };
        self.conversation_mix = damp(self.conversation_mix, goal, 1.0 / CONVERSATION_TAU, dt);
        if mode != CameraMode::Fixed {
            self.fixed = None;
        }
        match mode {
            CameraMode::FirstPerson => {
                let eye = eye_position(target?, config.char_radius);
                let ahead = eye + look::forward(look.yaw, look.pitch);
                let fov = zoomed_fov(GAMEPLAY_FOV, look.offsets.zoom);
                Some(self.blend_conversation(eye, ahead, fov, look.yaw, config))
            }
            CameraMode::ThirdPerson => {
                let head = eye_position(target?, config.char_radius);
                let radius = THIRD_PERSON_RADIUS / look.offsets.zoom;
                let position = orbit_position(head, look.yaw, look.pitch, radius);
                Some(self.blend_conversation(position, head, GAMEPLAY_FOV, look.yaw, config))
            }
            // A tiny z keeps the look-down direction's up vector stable.
            CameraMode::TopDown => Some(pose(
                Vec3::new(0.0, TOP_DOWN_HEIGHT, 0.001),
                Vec3::ZERO,
                TOP_DOWN_FOV,
            )),
            CameraMode::FreeFly => {
                let fly = self.free_fly;
                let ahead = fly.pos + look::forward(fly.yaw, fly.pitch);
                Some(pose(fly.pos, ahead, FREE_FLY_FOV))
            }
            CameraMode::Fixed => {
                let reference = config.fixed.look_at.map(Vec3::from).or(target)?;
                Some(self.fixed(reference, config, look))
            }
        }
    }

    /// Blends a first- or third-person pose toward an elevated view of the
    /// conversation focus that keeps the user's yaw.
    fn blend_conversation(
        &self,
        position: Vec3,
        look_at: Vec3,
        fov: f32,
        yaw: f32,
        config: &CameraConfig,
    ) -> SceneCamera {
        let mix = self.conversation_mix;
        if mix <= 0.001 {
            return pose(position, look_at, fov);
        }
        let (distance, height) = (config.conversation_distance, config.conversation_height);
        let radius = distance.hypot(height);
        let pitch_down = height.atan2(distance.max(0.01));
        let convo = orbit_position(self.last_focus, yaw, -pitch_down, radius);
        pose(
            position.lerp(convo, mix),
            look_at.lerp(self.last_focus, mix),
            fov,
        )
    }

    /// A camera whose orientation comes only from the azimuth and pitch (and
    /// the view's orbit offsets), at a fixed height above `reference`, moved
    /// only when `reference` leaves the leash.
    fn fixed(&mut self, reference: Vec3, config: &CameraConfig, look: &mut Look) -> SceneCamera {
        let fixed = &config.fixed;
        let offsets = &mut look.offsets;
        let leash = fixed_leash(fixed.pitch_deg.to_radians(), fixed, offsets);
        offsets.fixed_pitch_offset_rad = leash.pitch_offset_rad;
        let az = fixed.azimuth_deg.to_radians() + offsets.fixed_azimuth_offset_rad;

        let state = self.fixed.get_or_insert_with(|| {
            let distance = (leash.min_distance + leash.max_distance) / 2.0;
            FixedPose {
                position: reference
                    + Vec3::new(az.sin() * distance, leash.height, -az.cos() * distance),
                applied_azimuth_offset: offsets.fixed_azimuth_offset_rad,
            }
        });
        let mut position = state.position;

        // An azimuth drag swings the camera around the reference.
        let d_az = offsets.fixed_azimuth_offset_rad - state.applied_azimuth_offset;
        if d_az != 0.0 {
            state.applied_azimuth_offset = offsets.fixed_azimuth_offset_rad;
            let o = Vec2::new(position.x - reference.x, position.z - reference.z);
            position.x = reference.x + o.x * d_az.cos() - o.y * d_az.sin();
            position.z = reference.z + o.x * d_az.sin() + o.y * d_az.cos();
        }
        position.y = reference.y + leash.height;

        // Depth along the camera's forward axis and lateral along its right
        // axis are leashed independently.
        let forward = Vec2::new(-az.sin(), az.cos());
        let right = Vec2::new(-az.cos(), -az.sin());
        let to_reference = Vec2::new(reference.x - position.x, reference.z - position.z);
        let parallel = to_reference.dot(forward);
        let perp = to_reference.dot(right);
        let depth = parallel - parallel.clamp(leash.min_distance, leash.max_distance);
        let lateral = perp - perp.clamp(-leash.max_lateral, leash.max_lateral);
        let correction = forward * depth + right * lateral;
        position.x += correction.x;
        position.z += correction.y;

        let look_yaw = az + PI;
        let cos_p = leash.pitch_rad.cos();
        let mut look_at = position
            + Vec3::new(
                look_yaw.sin() * cos_p,
                leash.pitch_rad.sin(),
                -look_yaw.cos() * cos_p,
            );

        let mix = self.conversation_mix;
        if mix > 0.001 {
            // The same geometry anchored on the focus, closer, aimed at chest height.
            let focus = self.last_focus;
            let distance = config.conversation_distance;
            let convo = Vec3::new(
                focus.x - forward.x * distance,
                focus.y + config.conversation_height,
                focus.z - forward.y * distance,
            );
            position = position.lerp(convo, mix);
            look_at = look_at.lerp(focus + Vec3::Y * (CHAR_HEIGHT_M / 2.0), mix);
        }
        state.position = position;
        pose(position, look_at, fixed.fov)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::{ViewOffsets, forward};

    const EPS: f32 = 1e-3;

    fn close(a: impl Into<Vec3>, b: Vec3) {
        let a = a.into();
        assert!(a.distance(b) < EPS, "{a} != {b}");
    }

    fn step(rig: &mut Rig, mode: CameraMode, look: &mut Look, focus: Option<Vec3>) -> SceneCamera {
        let target = Some(Vec3::new(2.0, 1.0, -3.0));
        rig.step(
            mode,
            &CameraConfig::default(),
            look,
            target,
            focus,
            1.0 / 60.0,
        )
        .unwrap()
    }

    #[test]
    fn first_person_sits_at_the_eye_looking_along_the_view() {
        let mut look = Look {
            yaw: 0.7,
            pitch: -0.25,
            offsets: ViewOffsets {
                zoom: 3.0,
                ..ViewOffsets::default()
            },
        };
        let camera = step(
            &mut Rig::default(),
            CameraMode::FirstPerson,
            &mut look,
            None,
        );
        // Body root y 1 + BODY_Y 0.9 - charRadius 0.4 + EYE_HEIGHT 1.6.
        let eye = Vec3::new(2.0, 3.1, -3.0);
        close(camera.position, eye);
        close(camera.look_at, eye + forward(0.7, -0.25));
        assert!((camera.fov_deg - 30.0).abs() < EPS);
    }

    #[test]
    fn third_person_orbits_the_eye_at_six_metres_over_the_zoom() {
        let mut look = Look {
            offsets: ViewOffsets {
                zoom: 2.0,
                ..ViewOffsets::default()
            },
            ..Look::default()
        };
        let camera = step(
            &mut Rig::default(),
            CameraMode::ThirdPerson,
            &mut look,
            None,
        );
        let eye = Vec3::new(2.0, 3.1, -3.0);
        close(camera.look_at, eye);
        close(camera.position, orbit_position(eye, 0.0, -0.25, 3.0));
        assert!(Vec3::from(camera.position).y > eye.y);
        assert_eq!(camera.fov_deg, GAMEPLAY_FOV);
    }

    #[test]
    fn modes_that_follow_wait_for_a_target() {
        let config = CameraConfig::default();
        let mut look = Look::default();
        for mode in [
            CameraMode::FirstPerson,
            CameraMode::ThirdPerson,
            CameraMode::Fixed,
        ] {
            assert!(
                Rig::default()
                    .step(mode, &config, &mut look, None, None, 0.0)
                    .is_none()
            );
        }
        for mode in [CameraMode::TopDown, CameraMode::FreeFly] {
            assert!(
                Rig::default()
                    .step(mode, &config, &mut look, None, None, 0.0)
                    .is_some()
            );
        }
    }

    #[test]
    fn top_down_looks_straight_down_at_the_origin() {
        let camera = step(
            &mut Rig::default(),
            CameraMode::TopDown,
            &mut Look::default(),
            None,
        );
        close(camera.position, Vec3::new(0.0, 60.0, 0.001));
        close(camera.look_at, Vec3::ZERO);
        assert_eq!(camera.fov_deg, 45.0);
    }

    #[test]
    fn free_fly_flies_dollies_and_turns() {
        let mut rig = Rig::default();
        let camera = step(&mut rig, CameraMode::FreeFly, &mut Look::default(), None);
        close(camera.position, Vec3::new(0.0, 25.0, 25.0));
        close(
            camera.look_at,
            Vec3::new(0.0, 25.0, 25.0) + forward(0.0, -0.7),
        );

        let mut fly = FreeFly::default();
        let keys = |axes, fast| FlyKeys { axes, fast };
        fly.fly(keys(Vec3::Z, false), 0.5);
        close(fly.pos, FreeFly::default().pos + forward(0.0, -0.7) * 6.0);
        fly.fly(keys(Vec3::Y, true), 0.5);
        close(
            fly.pos,
            FreeFly::default().pos + forward(0.0, -0.7) * 6.0 + Vec3::Y * 18.0,
        );

        let mut fly = FreeFly::default();
        fly.dolly(-100.0);
        close(fly.pos, FreeFly::default().pos + forward(0.0, -0.7) * 3.0);
        fly.drag(Vec2::new(100.0, 1000.0));
        assert!((fly.yaw + 0.5).abs() < EPS);
        assert_eq!(fly.pitch, PITCH_MIN);
    }

    #[test]
    fn fixed_starts_mid_leash_on_the_azimuth_and_follows_past_the_far_leash() {
        let config = CameraConfig::default();
        let fixed = &config.fixed;
        let leash = fixed_leash(fixed.pitch_deg.to_radians(), fixed, &ViewOffsets::default());
        let az = fixed.azimuth_deg.to_radians();
        let mid = (leash.min_distance + leash.max_distance) / 2.0;
        let target = Vec3::new(2.0, 1.0, -3.0);
        let start = target + Vec3::new(az.sin() * mid, leash.height, -az.cos() * mid);

        let mut rig = Rig::default();
        let mut look = Look::default();
        let camera = step(&mut rig, CameraMode::Fixed, &mut look, None);
        close(camera.position, start);
        assert_eq!(camera.fov_deg, fixed.fov);
        let dir = (Vec3::from(camera.look_at) - start).normalize();
        assert!((dir.y - leash.pitch_rad.sin()).abs() < EPS);

        // A small step stays inside the leash: the camera holds still.
        let near = target + Vec3::X * 0.5;
        let held = rig.step(CameraMode::Fixed, &config, &mut look, Some(near), None, 0.0);
        close(held.unwrap().position, start);

        // Walking far along the camera's forward axis drags it to the far leash.
        let forward = Vec3::new(-az.sin(), 0.0, az.cos());
        let far = target + forward * 100.0;
        let pulled = rig
            .step(CameraMode::Fixed, &config, &mut look, Some(far), None, 0.0)
            .unwrap();
        let depth = (far - Vec3::from(pulled.position)).dot(forward);
        assert!((depth - leash.max_distance).abs() < EPS);
    }

    #[test]
    fn fixed_swings_around_the_reference_on_an_azimuth_drag() {
        let mut rig = Rig::default();
        let mut look = Look::default();
        let before = step(&mut rig, CameraMode::Fixed, &mut look, None);
        look.offsets.fixed_azimuth_offset_rad = PI / 2.0;
        let after = step(&mut rig, CameraMode::Fixed, &mut look, None);
        let target = Vec3::new(2.0, 1.0, -3.0);
        let flat = |p: [f32; 3]| Vec2::new(p[0] - target.x, p[2] - target.z);
        assert!((flat(before.position).length() - flat(after.position).length()).abs() < EPS);
        assert!(flat(before.position).dot(flat(after.position)).abs() < 1e-2);
    }

    #[test]
    fn conversation_focus_blends_in_with_a_damped_mix_and_back_out() {
        let focus = Vec3::new(10.0, 0.0, 10.0);
        let mut rig = Rig::default();
        let mut look = Look::default();
        let config = CameraConfig::default();
        let target = Some(Vec3::new(2.0, 1.0, -3.0));
        let alone = rig
            .step(
                CameraMode::FirstPerson,
                &config,
                &mut look,
                target,
                None,
                0.35,
            )
            .unwrap();
        let blended = rig
            .step(
                CameraMode::FirstPerson,
                &config,
                &mut look,
                target,
                Some(focus),
                0.35,
            )
            .unwrap();
        // One time constant in: 1 - 1/e of the way there.
        let mix = 1.0 - (-1.0f32).exp();
        let (distance, height) = (7.0f32, 5.0f32);
        let convo = orbit_position(focus, 0.0, -height.atan2(distance), distance.hypot(height));
        close(
            blended.position,
            Vec3::from(alone.position).lerp(convo, mix),
        );
        close(blended.look_at, Vec3::from(alone.look_at).lerp(focus, mix));

        // Settled, the camera sits on the conversation pose.
        let settled = rig
            .step(
                CameraMode::FirstPerson,
                &config,
                &mut look,
                target,
                Some(focus),
                10.0,
            )
            .unwrap();
        close(settled.position, convo);

        // Focus gone, it blends back from the last focus.
        let leaving = rig
            .step(
                CameraMode::FirstPerson,
                &config,
                &mut look,
                target,
                None,
                0.35,
            )
            .unwrap();
        close(
            leaving.position,
            convo.lerp(Vec3::from(alone.position), mix),
        );
    }

    #[test]
    fn fixed_conversation_frames_the_focus_from_the_azimuth() {
        let focus = Vec3::new(10.0, 0.0, 10.0);
        let mut rig = Rig::default();
        let camera = rig
            .step(
                CameraMode::Fixed,
                &CameraConfig::default(),
                &mut Look::default(),
                Some(focus),
                Some(focus),
                10.0,
            )
            .unwrap();
        let az = 202.5f32.to_radians();
        let forward = Vec3::new(-az.sin(), 0.0, az.cos());
        close(camera.position, focus - forward * 7.0 + Vec3::Y * 5.0);
        close(camera.look_at, focus + Vec3::Y * 0.9);
    }
}
