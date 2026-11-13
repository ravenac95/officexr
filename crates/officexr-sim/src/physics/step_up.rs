//! Ledge climbing for kinematic characters: lift, carry forward past the
//! edge, drop back down, and accept only a grounded landing higher than the
//! start. Rapier's autostep can't lift a ball over a real riser.

use crate::world_objects::Vec3;

/// At least the ball radius plus a margin, so the contact foot lands on the tread.
pub const STEP_UP_FORWARD_CLEARANCE: f64 = 0.45;
/// Above the 0.5 m staircase risers, below a 2 m block.
pub const STEP_UP_MAX_HEIGHT: f64 = 0.6;
const MIN_LANDED_RISE: f64 = 0.05;
const MIN_FORWARD_FRACTION: f64 = 0.5;

pub struct StepUpComputeResult {
    pub movement: Vec3,
    pub grounded: bool,
}

pub trait StepUpProbe {
    fn compute(&mut self, desired: Vec3) -> StepUpComputeResult;
    fn translation(&self) -> Vec3;
    /// Must move the colliders too, so later `compute` calls see the pose.
    fn set_translation(&mut self, p: Vec3);
    /// Whether the last `compute` touched another character. Checked per
    /// probe: a contact-clamped peer can block a sweep without appearing in
    /// the step's own collision list.
    fn last_contact_is_character(&self) -> bool;
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// The delta that climbs a ledge in the direction of `(dx, dz)`, or `None`
/// when there is none. The probe's pose is always restored.
pub fn try_step_up(probe: &mut impl StepUpProbe, dx: f64, dz: f64) -> Option<Vec3> {
    let max_height = STEP_UP_MAX_HEIGHT;
    let len = dx.hypot(dz);
    if len < 1e-9 {
        return None;
    }
    let start = probe.translation();

    let up = probe
        .compute(Vec3 {
            x: 0.0,
            y: max_height,
            z: 0.0,
        })
        .movement;
    if up.y < max_height - 0.01 {
        return None;
    }
    probe.set_translation(add(start, up));

    let dist = len.max(STEP_UP_FORWARD_CLEARANCE);
    let fwd = probe
        .compute(Vec3 {
            x: dx / len * dist,
            y: 0.0,
            z: dz / len * dist,
        })
        .movement;
    if fwd.x.hypot(fwd.z) < dist * MIN_FORWARD_FRACTION || probe.last_contact_is_character() {
        probe.set_translation(start);
        return None;
    }
    let lifted = probe.translation();
    probe.set_translation(add(lifted, fwd));

    let down = probe.compute(Vec3 {
        x: 0.0,
        y: -(max_height + 0.05),
        z: 0.0,
    });
    let landed_on_character = probe.last_contact_is_character();
    probe.set_translation(start);
    let landed_rise = up.y + fwd.y + down.movement.y;
    if !down.grounded || landed_on_character || landed_rise < MIN_LANDED_RISE {
        return None;
    }
    Some(Vec3 {
        x: up.x + fwd.x + down.movement.x,
        y: landed_rise,
        z: up.z + fwd.z + down.movement.z,
    })
}
