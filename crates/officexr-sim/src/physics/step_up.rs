//! Ledge climbing for kinematic characters: lift, probe forward past the
//! edge, drop back down, and accept only a grounded landing higher than the
//! start. The hop itself rises vertically and advances only the walk
//! distance. Rapier's autostep can't lift a ball over a real riser.

use crate::world_objects::Vec3;

/// How far the forward probe reaches: the ball radius plus a margin, so the
/// contact foot lands on the tread. Never applied as movement.
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
    // Vertical hop plus the swept advance, capped at the walk length.
    let scale = (len / fwd.x.hypot(fwd.z)).min(1.0);
    Some(Vec3 {
        x: fwd.x * scale,
        y: landed_rise,
        z: fwd.z * scale,
    })
}

/// Whether something lies within the probe reach below the ball. A corner
/// makes the sweep slide, so any deviation from the straight drop counts.
pub fn has_support(probe: &mut impl StepUpProbe) -> bool {
    let m = probe
        .compute(Vec3 {
            x: 0.0,
            y: -STEP_UP_FORWARD_CLEARANCE,
            z: 0.0,
        })
        .movement;
    m.x.hypot(m.z).hypot(m.y + STEP_UP_FORWARD_CLEARANCE) > 0.01
}

/// Keeps gravity off after a hop until the ball rests on the tread. The hop
/// only advances the walk distance, so the ball sits at tread height short of
/// the edge; gravity would slide it back down the riser at slow speeds.
/// The hold ends on grounding, losing support, being blocked, stopping, or
/// once the ball has walked the probe reach.
#[derive(Debug, Default)]
pub struct StepUpHold {
    remaining: f64,
}

impl StepUpHold {
    pub fn begin(&mut self) {
        self.remaining = STEP_UP_FORWARD_CLEARANCE;
    }

    pub fn reset(&mut self) {
        self.remaining = 0.0;
    }

    pub fn active(&self) -> bool {
        self.remaining > 0.0
    }

    /// Call after each sweep: `walked` of the `intended` horizontal distance was covered.
    pub fn update(&mut self, walked: f64, intended: f64, grounded: bool, supported: bool) {
        let blocked = walked < intended * 0.1;
        self.remaining = if grounded || !supported || intended == 0.0 || blocked {
            0.0
        } else {
            (self.remaining - walked).max(0.0)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Free space with a 0.5 m ledge ahead; `character` marks every contact as a peer.
    struct LedgeProbe {
        pos: Vec3,
        character: bool,
    }

    impl StepUpProbe for LedgeProbe {
        fn compute(&mut self, desired: Vec3) -> StepUpComputeResult {
            let landing = if desired.y < 0.0 { -0.1 } else { desired.y };
            StepUpComputeResult {
                movement: Vec3 {
                    y: landing,
                    ..desired
                },
                grounded: desired.y < 0.0,
            }
        }
        fn translation(&self) -> Vec3 {
            self.pos
        }
        fn set_translation(&mut self, p: Vec3) {
            self.pos = p;
        }
        fn last_contact_is_character(&self) -> bool {
            self.character
        }
    }

    #[test]
    fn step_up_hop_advances_only_the_walk_distance() {
        let mut ledge = LedgeProbe {
            pos: Vec3::default(),
            character: false,
        };
        let hop = try_step_up(&mut ledge, -0.05, 0.0).expect("ledge");
        assert!(hop.y > 0.0);
        assert!(hop.x.hypot(hop.z) <= 0.05 + 0.01);
    }

    #[test]
    fn climbs_a_ledge_but_never_a_character() {
        let mut ledge = LedgeProbe {
            pos: Vec3::default(),
            character: false,
        };
        assert!(try_step_up(&mut ledge, 0.05, 0.0).is_some_and(|d| d.y > 0.0));
        assert_eq!(ledge.pos, Vec3::default());

        let mut peer = LedgeProbe {
            pos: Vec3::default(),
            character: true,
        };
        assert_eq!(try_step_up(&mut peer, 0.05, 0.0), None);
        assert_eq!(peer.pos, Vec3::default());
    }
}
