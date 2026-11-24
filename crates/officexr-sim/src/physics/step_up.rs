//! Ledge climbing for kinematic characters: lift, probe forward past the
//! edge, drop back down, and accept only a grounded landing higher than the
//! start. The validated path is then walked rise-then-forward at a constant
//! pace by `StepUpTraversal`. Rapier's autostep can't lift a ball over a real
//! riser.

use crate::physics::character_movement::XZ;
use crate::world_objects::Vec3;

/// How far the forward probe reaches: the ball radius plus a margin, so the
/// contact foot lands on the tread.
pub const STEP_UP_FORWARD_CLEARANCE: f64 = 0.45;
/// Above the 0.5 m staircase risers, below a 2 m block.
pub const STEP_UP_MAX_HEIGHT: f64 = 0.6;
const MIN_LANDED_RISE: f64 = 0.05;
const MIN_FORWARD_FRACTION: f64 = 0.5;
/// Below this fraction of a slice surviving the caller's sweep, the climb is
/// treated as obstructed and abandoned.
const MIN_SLICE_FRACTION: f64 = 0.5;

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

/// A validated step relative to the character: rise straight up by `rise`,
/// then carry horizontally by `forward`. Walking rise-then-forward instead of
/// the probes' up/forward/down keeps Y from dipping mid-step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepUpPath {
    pub rise: f64,
    pub forward: XZ,
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// The path onto a ledge in the direction of `(dx, dz)`, or `None` when
/// there is none. The probe's pose is always restored.
pub fn try_step_up(probe: &mut impl StepUpProbe, dx: f64, dz: f64) -> Option<StepUpPath> {
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
    Some(StepUpPath {
        rise: landed_rise,
        forward: XZ {
            x: up.x + fwd.x + down.movement.x,
            z: up.z + fwd.z + down.movement.z,
        },
    })
}

/// Walks a `StepUpPath` at a constant `speed` (m/s along the path).
#[derive(Debug, Clone)]
pub struct StepUpTraversal {
    path: StepUpPath,
    pub speed: f64,
    /// Unit XZ direction of the forward carry.
    pub forward_dir: XZ,
    forward_len: f64,
    travelled: f64,
}

impl StepUpTraversal {
    pub fn new(path: StepUpPath, speed: f64) -> Self {
        let forward_len = path.forward.x.hypot(path.forward.z);
        let forward_dir = if forward_len > 0.0 {
            XZ {
                x: path.forward.x / forward_len,
                z: path.forward.z / forward_len,
            }
        } else {
            XZ::default()
        };
        Self {
            path,
            speed,
            forward_dir,
            forward_len,
            travelled: 0.0,
        }
    }

    /// Horizontal climb velocity: `forward_dir × speed`.
    pub fn velocity(&self) -> XZ {
        XZ {
            x: self.forward_dir.x * self.speed,
            z: self.forward_dir.z * self.speed,
        }
    }

    /// Moves `speed·dt_sec` further along the path (clamped to its end) and
    /// returns that slice, plus whether this call reached the end.
    pub fn advance(&mut self, dt_sec: f64) -> (Vec3, bool) {
        let length = self.path.rise + self.forward_len;
        let from = self.point_at(self.travelled);
        self.travelled = (self.travelled + self.speed * dt_sec).min(length);
        let to = self.point_at(self.travelled);
        (
            Vec3 {
                x: to.x - from.x,
                y: to.y - from.y,
                z: to.z - from.z,
            },
            self.travelled >= length,
        )
    }

    fn point_at(&self, s: f64) -> Vec3 {
        let carried = (s - self.path.rise).max(0.0);
        Vec3 {
            x: self.forward_dir.x * carried,
            y: s.min(self.path.rise),
            z: self.forward_dir.z * carried,
        }
    }
}

/// One frame of a climb: advances `traversal`, sweeps the slice through the
/// caller's controller, and returns the swept delta plus whether the climb
/// ended, either finished or cut short by the sweep. On an end the caller
/// drops the traversal and resumes normal movement and gravity.
pub fn step_traversal(
    traversal: &mut StepUpTraversal,
    dt_sec: f64,
    mut sweep: impl FnMut(Vec3) -> Vec3,
) -> (Vec3, bool) {
    let (requested, done) = traversal.advance(dt_sec);
    let delta = sweep(requested);
    let requested_sq = requested.x.powi(2) + requested.y.powi(2) + requested.z.powi(2);
    let survived = if requested_sq > 0.0 {
        (delta.x * requested.x + delta.y * requested.y + delta.z * requested.z) / requested_sq
    } else {
        1.0
    };
    (delta, done || survived < MIN_SLICE_FRACTION)
}

/// What a climbing character broadcasts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimbBroadcast {
    pub vel: Vec3,
    pub yaw: f64,
}

/// The climb velocity scaled by `climb_anim_factor` (it drives the walk
/// animation's rate), facing along the climb, or `current_yaw` when the
/// climb has no horizontal carry.
pub fn climb_broadcast(climb_vel: XZ, climb_anim_factor: f64, current_yaw: f64) -> ClimbBroadcast {
    let XZ { x, z } = climb_vel;
    ClimbBroadcast {
        vel: Vec3 {
            x: x * climb_anim_factor,
            y: 0.0,
            z: z * climb_anim_factor,
        },
        yaw: if x != 0.0 || z != 0.0 {
            (-x).atan2(-z)
        } else {
            current_yaw
        },
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
    fn the_path_sums_the_probe_moves() {
        let mut ledge = LedgeProbe {
            pos: Vec3::default(),
            character: false,
        };
        let path = try_step_up(&mut ledge, -0.05, 0.0).expect("ledge");
        assert!((path.rise - 0.5).abs() < 1e-12);
        assert!((path.forward.x + STEP_UP_FORWARD_CLEARANCE).abs() < 1e-12);
        assert_eq!(path.forward.z, 0.0);
    }

    #[test]
    fn climbs_a_ledge_but_never_a_character() {
        let mut ledge = LedgeProbe {
            pos: Vec3::default(),
            character: false,
        };
        assert!(try_step_up(&mut ledge, 0.05, 0.0).is_some_and(|p| p.rise > 0.0));
        assert_eq!(ledge.pos, Vec3::default());

        let mut peer = LedgeProbe {
            pos: Vec3::default(),
            character: true,
        };
        assert_eq!(try_step_up(&mut peer, 0.05, 0.0), None);
        assert_eq!(peer.pos, Vec3::default());
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    const PATH: StepUpPath = StepUpPath {
        rise: 0.5,
        forward: XZ { x: -0.3, z: 0.4 },
    };

    /// Advances until done; the slices and each call's done flag.
    fn walk(traversal: &mut StepUpTraversal, dt: f64) -> (Vec<Vec3>, Vec<bool>) {
        let (mut deltas, mut dones) = (vec![], vec![]);
        for _ in 0..1000 {
            let (delta, done) = traversal.advance(dt);
            deltas.push(delta);
            dones.push(done);
            if done {
                break;
            }
        }
        (deltas, dones)
    }

    #[test]
    fn a_traversal_sums_exactly_to_the_path_in_slices_no_longer_than_speed_dt() {
        let (speed, dt) = (1.4, 1.0 / 60.0);
        let (deltas, dones) = walk(&mut StepUpTraversal::new(PATH, speed), dt);
        let sum = deltas.iter().fold(Vec3::default(), |a, d| add(a, *d));
        assert!(close(sum.x, PATH.forward.x));
        assert!(close(sum.y, PATH.rise));
        assert!(close(sum.z, PATH.forward.z));
        for d in &deltas {
            assert!(d.x.hypot(d.y).hypot(d.z) <= speed * dt + 1e-12);
        }
        assert_eq!(dones.iter().filter(|d| **d).count(), 1);
        assert_eq!(dones.last(), Some(&true));
    }

    #[test]
    fn a_traversal_rises_before_it_carries_forward_crossing_within_one_call() {
        // 0.3 m of travel per call: call 1 rises 0.3, call 2 rises the last
        // 0.2 then carries 0.1 forward.
        let mut traversal = StepUpTraversal::new(PATH, 0.3);
        let (first, _) = traversal.advance(1.0);
        assert_eq!(
            first,
            Vec3 {
                x: 0.0,
                y: 0.3,
                z: 0.0
            }
        );
        let (second, _) = traversal.advance(1.0);
        assert!(close(second.y, 0.2));
        assert!(close(second.x.hypot(second.z), 0.1));
        assert!(close(second.x / 0.1, traversal.forward_dir.x));
        assert!(close(second.z / 0.1, traversal.forward_dir.z));
    }

    #[test]
    fn a_traversal_velocity_is_the_forward_direction_at_climb_speed() {
        let v = StepUpTraversal::new(PATH, 2.0).velocity();
        assert!(close(v.x, -1.2));
        assert!(close(v.z, 1.6));
        let vertical = StepUpPath {
            rise: 0.5,
            forward: XZ::default(),
        };
        assert_eq!(
            StepUpTraversal::new(vertical, 2.0).velocity(),
            XZ::default()
        );
    }

    #[test]
    fn a_dt_covering_the_whole_path_finishes_in_one_call() {
        let (deltas, dones) = walk(&mut StepUpTraversal::new(PATH, 1.0), 10.0);
        assert_eq!(dones, [true]);
        assert!(close(deltas[0].y, PATH.rise));
        assert!(close(deltas[0].x, PATH.forward.x));
        assert!(close(deltas[0].z, PATH.forward.z));
    }

    const FORWARD_PATH: StepUpPath = StepUpPath {
        rise: 0.5,
        forward: XZ { x: 0.45, z: 0.0 },
    };

    fn scaled(f: f64) -> impl FnMut(Vec3) -> Vec3 {
        move |d| Vec3 {
            x: d.x * f,
            y: d.y * f,
            z: d.z * f,
        }
    }

    #[test]
    fn step_traversal_returns_the_swept_slice_and_keeps_going_while_clear() {
        let mut traversal = StepUpTraversal::new(FORWARD_PATH, 1.4);
        let (delta, ended) = step_traversal(&mut traversal, 1.0 / 60.0, |d| d);
        assert!(close(delta.y, 1.4 / 60.0));
        assert!(!ended);
    }

    #[test]
    fn step_traversal_ends_when_the_sweep_keeps_under_half_the_slice() {
        let mut traversal = StepUpTraversal::new(FORWARD_PATH, 1.4);
        assert!(!step_traversal(&mut traversal, 1.0 / 60.0, scaled(0.6)).1);
        let (delta, ended) = step_traversal(&mut traversal, 1.0 / 60.0, scaled(0.4));
        assert!(ended);
        assert!(close(delta.y, 0.4 * 1.4 / 60.0));
    }

    #[test]
    fn step_traversal_ends_on_the_call_that_finishes_the_path() {
        let mut traversal = StepUpTraversal::new(FORWARD_PATH, 1.0);
        assert!(step_traversal(&mut traversal, 10.0, |d| d).1);
    }

    #[test]
    fn climb_broadcast_scales_the_climb_velocity_and_faces_along_it() {
        let b = climb_broadcast(XZ { x: -1.0, z: 0.0 }, 1.5, 0.3);
        assert_eq!(
            b.vel,
            Vec3 {
                x: -1.5,
                y: 0.0,
                z: 0.0
            }
        );
        assert!(close(b.yaw, std::f64::consts::FRAC_PI_2));
    }

    #[test]
    fn climb_broadcast_keeps_the_current_yaw_without_horizontal_carry() {
        assert_eq!(climb_broadcast(XZ::default(), 1.0, 0.3).yaw, 0.3);
    }
}
