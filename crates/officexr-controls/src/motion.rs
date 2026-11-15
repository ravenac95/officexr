//! Device-motion look, as `motionLook.ts`: the host pushes `DeviceOrientationEvent` readings and
//! each turns the view by the change since the previous one.

use serde::Deserialize;

/// Drops the 180° yaw flip as pitch crosses the zenith or nadir, and genuine sensor spikes; hand
/// motion stays well under this per reading.
pub const GLITCH_THRESHOLD_DEG: f64 = 30.0;

/// A `DeviceOrientationEvent`'s angles in degrees; desktop browsers send nulls.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct DeviceReading {
    pub alpha: Option<f64>,
    pub beta: Option<f64>,
    pub gamma: Option<f64>,
}

/// Where the device's back camera points, in degrees (positive yaw = left, positive pitch = up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionPose {
    pub yaw_deg: f64,
    pub pitch_deg: f64,
}

/// Where the back camera, `R(α,β,γ)·(0,0,−1)`, points. Rolling the device about its screen
/// normal leaves it unchanged, so portrait and landscape need no special-casing.
pub fn to_pose(reading: DeviceReading) -> Option<MotionPose> {
    let (a, b, g) = (
        reading.alpha?.to_radians(),
        reading.beta?.to_radians(),
        reading.gamma?.to_radians(),
    );
    let north = -a.sin() * g.sin() + a.cos() * b.sin() * g.cos();
    let west = a.cos() * g.sin() + a.sin() * b.sin() * g.cos();
    let up = (-b.cos() * g.cos()).clamp(-1.0, 1.0);
    Some(MotionPose {
        yaw_deg: west.atan2(north).to_degrees(),
        pitch_deg: up.asin().to_degrees(),
    })
}

fn wrap180(deg: f64) -> f64 {
    if deg > 180.0 {
        deg - 360.0
    } else if deg < -180.0 {
        deg + 360.0
    } else {
        deg
    }
}

fn unless_glitch(delta_deg: f64) -> f32 {
    if delta_deg.abs() <= GLITCH_THRESHOLD_DEG {
        delta_deg.to_radians() as f32
    } else {
        0.0
    }
}

/// The look delta `(d_yaw, d_pitch)` in radians, in `apply_look_delta`'s convention, between two
/// consecutive poses. An axis whose jump exceeds the glitch threshold contributes nothing.
pub fn look_delta(prev: MotionPose, next: MotionPose) -> (f32, f32) {
    (
        unless_glitch(wrap180(next.yaw_deg - prev.yaw_deg)),
        unless_glitch(next.pitch_deg - prev.pitch_deg),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    type Matrix = [[f64; 3]; 3];

    fn rot_x(deg: f64) -> Matrix {
        let (s, c) = deg.to_radians().sin_cos();
        [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
    }

    fn rot_z(deg: f64) -> Matrix {
        let (s, c) = deg.to_radians().sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    }

    fn mul(a: Matrix, b: Matrix) -> Matrix {
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        out
    }

    /// The alpha/beta/gamma a browser reports for a device rotation matrix (W3C Z-X'-Y'' order).
    fn reading_of(r: Matrix) -> DeviceReading {
        let sign = if r[2][2] < 0.0 { -1.0 } else { 1.0 };
        let cos_beta = sign * r[2][0].hypot(r[2][2]);
        DeviceReading {
            alpha: Some(
                (-r[0][1] / cos_beta)
                    .atan2(r[1][1] / cos_beta)
                    .to_degrees()
                    .rem_euclid(360.0),
            ),
            beta: Some(r[2][1].atan2(cos_beta).to_degrees()),
            gamma: Some((-r[2][0] / cos_beta).atan2(r[2][2] / cos_beta).to_degrees()),
        }
    }

    /// A device whose back camera points `heading` left of north and `up` above the horizon,
    /// rolled `roll` about its screen normal (0 = portrait, 90 = landscape, ...).
    fn pose_of(heading: f64, up: f64, roll: f64) -> MotionPose {
        to_pose(reading_of(mul(
            rot_z(heading),
            mul(rot_x(90.0 + up), rot_z(roll)),
        )))
        .unwrap()
    }

    fn close(a: f32, b: f64) -> bool {
        (f64::from(a) - b).abs() < 1e-6
    }

    #[test]
    fn a_partial_reading_has_no_pose() {
        assert_eq!(to_pose(DeviceReading::default()), None);
        let partial = DeviceReading {
            alpha: Some(10.0),
            beta: Some(90.0),
            gamma: None,
        };
        assert_eq!(to_pose(partial), None);
    }

    #[test]
    fn turning_and_tilting_look_the_same_way_at_every_screen_roll() {
        for roll in [0.0, 90.0, 180.0, 270.0] {
            let before = pose_of(0.0, -10.0, roll);
            let (d_yaw, d_pitch) = look_delta(before, pose_of(-3.0, -10.0, roll));
            assert!(close(d_yaw, -3f64.to_radians()) && close(d_pitch, 0.0));
            let (d_yaw, d_pitch) = look_delta(before, pose_of(0.0, -7.0, roll));
            assert!(close(d_yaw, 0.0) && close(d_pitch, 3f64.to_radians()));
        }
    }

    #[test]
    fn yaw_wraps_across_180_and_glitches_are_dropped() {
        let pose = |yaw_deg, pitch_deg| MotionPose { yaw_deg, pitch_deg };
        let (d_yaw, _) = look_delta(pose(179.0, 0.0), pose(-179.0, 0.0));
        assert!(close(d_yaw, 2f64.to_radians()));
        assert_eq!(look_delta(pose(100.0, 0.0), pose(140.0, 40.0)), (0.0, 0.0));
    }
}
