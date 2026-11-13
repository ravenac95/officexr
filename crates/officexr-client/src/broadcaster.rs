//! Port of `PositionBroadcaster` (`packages/sdk/src/realtime/outbound/position-broadcaster.ts`):
//! the local player's `presence:position` cadence. Keep the two in step.

use officexr_sim::Vec3;

const DELTA_P: f64 = 0.05;
const DELTA_YAW: f64 = 0.05;
const MAX_HZ: f64 = 30.0;
const STOP_GRACE_MS: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f64,
    pub is_airborne: bool,
}

pub struct PositionBroadcaster {
    last_pos: Vec3,
    last_yaw: f64,
    last_t_ms: f64,
    last_airborne: bool,
    was_moving: bool,
    has_initial: bool,
    pending_stop_since_ms: Option<f64>,
}

impl Default for PositionBroadcaster {
    fn default() -> Self {
        Self {
            last_pos: Vec3::default(),
            last_yaw: 0.0,
            last_t_ms: f64::NEG_INFINITY,
            last_airborne: false,
            was_moving: false,
            has_initial: false,
            pending_stop_since_ms: None,
        }
    }
}

fn distance(a: Vec3, b: Vec3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

fn angle_delta(a: f64, b: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let d = (a - b).abs() % tau;
    if d > std::f64::consts::PI { tau - d } else { d }
}

impl PositionBroadcaster {
    /// Latches `me` as the baseline and returns the spawn announcement (zero velocity). The rate
    /// cap stays open, so the first real move isn't held back by it.
    pub fn announce(&mut self, me: Pose) -> Pose {
        self.latch(me);
        self.has_initial = true;
        Pose {
            vel: Vec3::default(),
            ..me
        }
    }

    /// The pose to send this frame, if any.
    pub fn flush(&mut self, me: Pose, now_ms: f64) -> Option<Pose> {
        if !self.has_initial {
            self.latch(me);
            self.has_initial = true;
            return None;
        }
        let d_pos = distance(me.pos, self.last_pos);
        let dt = now_ms - self.last_t_ms;
        let above_ceiling = dt < 1000.0 / MAX_HZ;
        let moved_enough = d_pos > DELTA_P || angle_delta(me.yaw, self.last_yaw) > DELTA_YAW;
        // A flipped airborne flag skips the delta gate so peers start the jump clip at once.
        let airborne_changed = me.is_airborne != self.last_airborne;

        if (moved_enough || airborne_changed) && !above_ceiling {
            let vel = if dt > 0.0 && dt.is_finite() {
                Vec3 {
                    x: (me.pos.x - self.last_pos.x) * 1000.0 / dt,
                    y: (me.pos.y - self.last_pos.y) * 1000.0 / dt,
                    z: (me.pos.z - self.last_pos.z) * 1000.0 / dt,
                }
            } else {
                me.vel
            };
            self.sent(me, now_ms, true);
            return Some(Pose { vel, ..me });
        }

        if self.was_moving && !moved_enough {
            let since = *self.pending_stop_since_ms.get_or_insert(now_ms);
            if now_ms - since >= STOP_GRACE_MS {
                self.sent(me, now_ms, false);
                return Some(Pose {
                    vel: Vec3::default(),
                    ..me
                });
            }
        }
        None
    }

    fn latch(&mut self, me: Pose) {
        self.last_pos = me.pos;
        self.last_yaw = me.yaw;
        self.last_airborne = me.is_airborne;
    }

    fn sent(&mut self, me: Pose, now_ms: f64, moving: bool) {
        self.latch(me);
        self.last_t_ms = now_ms;
        self.was_moving = moving;
        self.pending_stop_since_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64) -> Pose {
        Pose {
            pos: Vec3 { x, y: 0.0, z: 0.0 },
            vel: Vec3 {
                x: 3.0,
                y: 0.0,
                z: 0.0,
            },
            yaw: 0.0,
            is_airborne: false,
        }
    }

    #[test]
    fn the_first_flush_only_latches_the_baseline() {
        let mut b = PositionBroadcaster::default();
        assert_eq!(b.flush(at(0.0), 0.0), None);
        // Past the delta and, with no send yet, not rate capped; vel falls back to the pose's.
        let sent = b.flush(at(0.06), 1.0).unwrap();
        assert_eq!(sent.vel.x, 3.0);
    }

    #[test]
    fn sends_past_the_position_or_yaw_delta_only() {
        let mut b = PositionBroadcaster::default();
        b.announce(at(0.0));
        assert_eq!(b.flush(at(0.05), 0.0), None);
        assert!(b.flush(at(0.051), 0.0).is_some());
        let turned = Pose {
            yaw: 0.051 + std::f64::consts::TAU,
            ..at(0.051)
        };
        assert!(b.flush(turned, 100.0).is_some());
    }

    #[test]
    fn caps_the_rate_and_estimates_velocity_between_sends() {
        let mut b = PositionBroadcaster::default();
        b.announce(at(0.0));
        b.flush(at(0.1), 1000.0).unwrap();
        assert_eq!(b.flush(at(0.2), 1020.0), None);
        let sent = b.flush(at(0.3), 1040.0).unwrap();
        assert!((sent.vel.x - 5.0).abs() < 1e-9);
    }

    #[test]
    fn an_airborne_flip_sends_without_moving() {
        let mut b = PositionBroadcaster::default();
        b.announce(at(0.0));
        let jumped = Pose {
            is_airborne: true,
            ..at(0.0)
        };
        assert!(b.flush(jumped, 0.0).unwrap().is_airborne);
    }

    #[test]
    fn sends_one_zero_velocity_stop_after_the_grace() {
        let mut b = PositionBroadcaster::default();
        b.announce(at(0.0));
        b.flush(at(1.0), 0.0).unwrap();
        assert_eq!(b.flush(at(1.0), 50.0), None);
        assert_eq!(b.flush(at(1.0), 149.0), None);
        let stop = b.flush(at(1.0), 150.0).unwrap();
        assert_eq!(stop.vel, Vec3::default());
        assert_eq!(b.flush(at(1.0), 1000.0), None);
    }
}
