//! Proximity pairing and MeetingAreas, ported from the TS `PairTracker`
//! (`packages/sdk/src/spatial/pair-tracker.ts`) and `tickProximity`.
//!
//! The internal collections are insertion-ordered `Vec`s because the TS
//! `Map`/`Set` iteration order decides the order events are emitted in.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::physics::character_movement::XZ;
use crate::sim_core::SimPeer;
use crate::world_objects::Vec3;

/// If updates stop arriving, the projected position freezes at this age
/// instead of running away with stale velocity.
pub const EXTRAPOLATION_CAP_S: f64 = 0.1;

/// `pos` advanced by `vel` since `t_recv_ms`, capped at
/// [`EXTRAPOLATION_CAP_S`]; `None` (the local player) is always fresh.
pub fn extrapolate_pos(pos: Vec3, vel: Vec3, t_recv_ms: Option<f64>, now_ms: f64) -> Vec3 {
    let Some(t_recv) = t_recv_ms else {
        return pos;
    };
    let elapsed = EXTRAPOLATION_CAP_S.min((now_ms - t_recv) / 1000.0);
    Vec3 {
        x: pos.x + vel.x * elapsed,
        y: pos.y + vel.y * elapsed,
        z: pos.z + vel.z * elapsed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PairState {
    Entering,
    Entered,
    Exiting,
}

impl PairState {
    fn visibility_rank(self) -> u8 {
        match self {
            PairState::Entering => 1,
            PairState::Exiting => 2,
            PairState::Entered => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProximityEventKind {
    #[serde(rename = "proximity:entering")]
    Entering,
    #[serde(rename = "proximity:entered")]
    Entered,
    #[serde(rename = "proximity:exiting")]
    Exiting,
    #[serde(rename = "proximity:exited")]
    Exited,
}

/// A transition of a pair involving the local player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProximityEvent {
    pub kind: ProximityEventKind,
    pub other_id: String,
}

/// A voice or desk zone: a player inside one never pairs by proximity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceZone {
    pub min: XZ,
    pub max: XZ,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProximitySettings {
    pub char_radius: f64,
    pub proximity_radius: f64,
    pub proximity_outer_radius: f64,
    pub proximity_enter_debounce_ms: f64,
}

/// One frame of proximity input; the players travel separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProximityInput {
    pub voice_zones: Vec<VoiceZone>,
    pub self_id: String,
    pub settings: ProximitySettings,
    pub now_ms: f64,
    pub dt_sec: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeetingArea {
    pub id: String,
    pub members: BTreeSet<String>,
    pub center: XZ,
    pub radius: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerOutput {
    /// In emission order, for the caller to emit.
    pub events: Vec<ProximityEvent>,
    /// Keyed `a|b`, the smaller id first.
    pub pair_states: BTreeMap<String, PairState>,
    /// Players whose disc renders, at the strongest state visible to the local player.
    pub player_states: BTreeMap<String, PairState>,
    pub areas: Vec<MeetingArea>,
    /// The local player's area centre, if it is in one.
    pub conversation_focus: Option<XZ>,
}

struct PairRecord {
    key: String,
    state: PairState,
    /// When the current inner overlap began, if it is running.
    debounce_started_at_ms: Option<f64>,
}

struct Area {
    id: String,
    members: Vec<String>,
    center: XZ,
    radius: f64,
}

type Players<'a> = BTreeMap<&'a str, Vec3>;

/// Pair states and MeetingAreas carried from frame to frame.
#[derive(Default)]
pub struct PairTracker {
    pairs: Vec<PairRecord>,
    areas: Vec<Area>,
    area_counter: u32,
}

impl PairTracker {
    /// Advances one frame over `players`, whose order is the TS `Object.keys` order.
    pub fn tick(&mut self, players: &[SimPeer], input: &ProximityInput) -> TrackerOutput {
        let positions: Players = players.iter().map(|p| (p.id.as_str(), p.pos)).collect();
        let excluded = players_in_zones(players, &input.voice_zones);
        let self_id = input.self_id.as_str();
        let s = &input.settings;
        let mut events = Vec::new();

        let outer_limit = 2.0 * s.char_radius + s.proximity_outer_radius;
        let inner_limit = 2.0 * s.char_radius + s.proximity_radius;
        let small_buffer = s.char_radius * 2.0;

        self.pairs.retain(|r| {
            let (a, b) = split_key(&r.key);
            positions.contains_key(a) && positions.contains_key(b)
        });
        for area in &mut self.areas {
            area.members.retain(|m| positions.contains_key(m.as_str()));
        }
        // Excluded players leave before admission, so none of their pairs flips to `entered`.
        for i in 0..self.areas.len() {
            let leaving: Vec<String> = self.areas[i]
                .members
                .iter()
                .filter(|m| excluded.contains(m.as_str()))
                .cloned()
                .collect();
            for m in leaving {
                self.eject(i, &m, &mut events, self_id);
            }
        }

        let mut outer: Vec<String> = Vec::new();
        let mut inner: BTreeSet<String> = BTreeSet::new();
        for (i, a) in players.iter().enumerate() {
            for b in &players[i + 1..] {
                if excluded.contains(a.id.as_str()) || excluded.contains(b.id.as_str()) {
                    continue;
                }
                let d_sq = dist_sq(a.pos.x, a.pos.z, b.pos.x, b.pos.z);
                if d_sq <= outer_limit * outer_limit {
                    let key = pair_key(&a.id, &b.id);
                    if d_sq <= inner_limit * inner_limit {
                        inner.insert(key.clone());
                    }
                    outer.push(key);
                }
            }
        }

        for key in &outer {
            let idx = match self.pairs.iter().position(|r| &r.key == key) {
                Some(idx) => idx,
                None => {
                    self.pairs.push(PairRecord {
                        key: key.clone(),
                        state: PairState::Entering,
                        debounce_started_at_ms: None,
                    });
                    emit(&mut events, self_id, key, ProximityEventKind::Entering);
                    self.pairs.len() - 1
                }
            };
            let rec = &mut self.pairs[idx];
            rec.debounce_started_at_ms = if rec.state == PairState::Entering && inner.contains(key)
            {
                rec.debounce_started_at_ms.or(Some(input.now_ms))
            } else {
                None
            };
        }

        // Admission can append pairs; they are `entered`, so the loop skips them.
        let mut i = 0;
        while i < self.pairs.len() {
            let rec = &mut self.pairs[i];
            i += 1;
            let Some(started) = rec.debounce_started_at_ms else {
                continue;
            };
            if rec.state != PairState::Entering
                || input.now_ms - started < s.proximity_enter_debounce_ms
            {
                continue;
            }
            if !inner.contains(&rec.key) {
                rec.debounce_started_at_ms = None;
                continue;
            }
            let (a, b) = split_key(&rec.key);
            let (a, b) = (a.to_owned(), b.to_owned());
            let admitted = match (self.area_containing(&a), self.area_containing(&b)) {
                (None, None) => self.create_area(&a, &b, &positions, small_buffer),
                (Some(ia), None) => self.add_member(ia, &b, &positions, small_buffer),
                (None, Some(ib)) => self.add_member(ib, &a, &positions, small_buffer),
                (Some(ia), Some(ib)) if ia == ib => ia,
                (Some(ia), Some(ib)) => self.merge_areas(ia, ib, &positions, small_buffer),
            };
            self.flip_pairs_to_entered(admitted, &mut events, self_id);
        }

        let mut i = 0;
        while i < self.areas.len() {
            if self.areas[i].members.is_empty() {
                self.areas.remove(i);
                continue;
            }
            let area = &mut self.areas[i];
            let centroid = centroid(&area.members, &positions);
            area.center.x = damp(area.center.x, centroid.x, 0.5, input.dt_sec);
            area.center.z = damp(area.center.z, centroid.z, 0.5, input.dt_sec);
            let ejected: Vec<String> = area
                .members
                .iter()
                .filter(|m| {
                    let p = positions[m.as_str()];
                    dist_sq(p.x, p.z, area.center.x, area.center.z) > area.radius * area.radius
                })
                .cloned()
                .collect();
            for m in ejected {
                self.eject(i, &m, &mut events, self_id);
            }
            if self.areas[i].members.len() <= 1 {
                self.areas.remove(i);
            } else {
                i += 1;
            }
        }

        self.pairs.retain(|r| {
            let kept = outer.contains(&r.key);
            if !kept {
                emit(&mut events, self_id, &r.key, ProximityEventKind::Exited);
            }
            kept
        });

        let area_of = |p: &str| self.area_containing(p);
        let mut pair_states = BTreeMap::new();
        let mut player_states: BTreeMap<String, PairState> = BTreeMap::new();
        for rec in &self.pairs {
            pair_states.insert(rec.key.clone(), rec.state);
            let (a, b) = split_key(&rec.key);
            let in_same_area = area_of(a).is_some_and(|x| area_of(b) == Some(x));
            let self_involved = a == self_id || b == self_id;
            if !self_involved && !(in_same_area && rec.state != PairState::Entering) {
                continue;
            }
            for p in [a, b] {
                let cur = player_states.entry(p.to_owned()).or_insert(rec.state);
                if rec.state.visibility_rank() > cur.visibility_rank() {
                    *cur = rec.state;
                }
            }
        }

        TrackerOutput {
            events,
            pair_states,
            player_states,
            areas: self
                .areas
                .iter()
                .map(|a| MeetingArea {
                    id: a.id.clone(),
                    members: a.members.iter().cloned().collect(),
                    center: a.center,
                    radius: a.radius,
                })
                .collect(),
            conversation_focus: area_of(self_id).map(|i| self.areas[i].center),
        }
    }

    /// Removes `m` from area `idx`; its `entered` pairs with the remaining members go to `exiting`.
    fn eject(&mut self, idx: usize, m: &str, events: &mut Vec<ProximityEvent>, self_id: &str) {
        let area = &mut self.areas[idx];
        area.members.retain(|x| x != m);
        for other in &area.members {
            let key = pair_key(m, other);
            if let Some(rec) = self.pairs.iter_mut().find(|r| r.key == key)
                && rec.state == PairState::Entered
            {
                rec.state = PairState::Exiting;
                emit(events, self_id, &key, ProximityEventKind::Exiting);
            }
        }
    }

    fn area_containing(&self, p: &str) -> Option<usize> {
        self.areas
            .iter()
            .position(|a| a.members.iter().any(|m| m == p))
    }

    fn create_area(&mut self, a: &str, b: &str, positions: &Players, small_buffer: f64) -> usize {
        self.area_counter += 1;
        let (pa, pb) = (positions[a], positions[b]);
        let center = XZ {
            x: (pa.x + pb.x) / 2.0,
            z: (pa.z + pb.z) / 2.0,
        };
        let reach = dist(pa.x, pa.z, center.x, center.z).max(dist(pb.x, pb.z, center.x, center.z));
        self.areas.push(Area {
            id: format!("area-{}", self.area_counter),
            members: vec![a.to_owned(), b.to_owned()],
            center,
            radius: reach + small_buffer,
        });
        self.areas.len() - 1
    }

    fn add_member(
        &mut self,
        idx: usize,
        id: &str,
        positions: &Players,
        small_buffer: f64,
    ) -> usize {
        let area = &mut self.areas[idx];
        if !area.members.iter().any(|m| m == id) {
            area.members.push(id.to_owned());
            refit(area, area.radius, positions, small_buffer);
        }
        idx
    }

    /// Folds the smaller area into the larger, ties to `a`; returns the kept area's index.
    fn merge_areas(&mut self, a: usize, b: usize, positions: &Players, small_buffer: f64) -> usize {
        let (keep, drop) = if self.areas[a].members.len() >= self.areas[b].members.len() {
            (a, b)
        } else {
            (b, a)
        };
        let dropped = self.areas.remove(drop);
        let keep = if drop < keep { keep - 1 } else { keep };
        let area = &mut self.areas[keep];
        area.members.extend(dropped.members);
        let floor = area.radius.max(dropped.radius);
        refit(area, floor, positions, small_buffer);
        keep
    }

    /// Flips every `entering` (or untracked) pair inside area `idx` to `entered`.
    fn flip_pairs_to_entered(
        &mut self,
        idx: usize,
        events: &mut Vec<ProximityEvent>,
        self_id: &str,
    ) {
        let members = self.areas[idx].members.clone();
        for (i, a) in members.iter().enumerate() {
            for b in &members[i + 1..] {
                let key = pair_key(a, b);
                match self.pairs.iter_mut().find(|r| r.key == key) {
                    Some(rec) if rec.state == PairState::Entered => {}
                    Some(rec) => {
                        rec.state = PairState::Entered;
                        rec.debounce_started_at_ms = None;
                        emit(events, self_id, &key, ProximityEventKind::Entered);
                    }
                    // A merge can bridge members that never outer-overlapped.
                    None => {
                        emit(events, self_id, &key, ProximityEventKind::Entered);
                        self.pairs.push(PairRecord {
                            key,
                            state: PairState::Entered,
                            debounce_started_at_ms: None,
                        });
                    }
                }
            }
        }
    }
}

/// Snaps the centre to the members' centroid and grows the radius to fit
/// them, never below `floor`.
fn refit(area: &mut Area, floor: f64, positions: &Players, small_buffer: f64) {
    let c = centroid(&area.members, positions);
    let reach = area
        .members
        .iter()
        .map(|m| {
            let p = positions[m.as_str()];
            dist(p.x, p.z, c.x, c.z)
        })
        .fold(0.0, f64::max);
    area.center = c;
    area.radius = floor.max(reach + small_buffer);
}

fn centroid(members: &[String], positions: &Players) -> XZ {
    let (mut x, mut z) = (0.0, 0.0);
    for m in members {
        let p = positions[m.as_str()];
        x += p.x;
        z += p.z;
    }
    let n = members.len() as f64;
    XZ { x: x / n, z: z / n }
}

/// Players standing in any zone, bounds inclusive.
fn players_in_zones<'a>(players: &'a [SimPeer], zones: &[VoiceZone]) -> BTreeSet<&'a str> {
    players
        .iter()
        .filter(|p| {
            zones.iter().any(|r| {
                p.pos.x >= r.min.x && p.pos.x <= r.max.x && p.pos.z >= r.min.z && p.pos.z <= r.max.z
            })
        })
        .map(|p| p.id.as_str())
        .collect()
}

fn pair_key(a: &str, b: &str) -> String {
    if a < b {
        format!("{a}|{b}")
    } else {
        format!("{b}|{a}")
    }
}

fn split_key(key: &str) -> (&str, &str) {
    key.split_once('|').expect("pair keys hold a `|`")
}

fn dist_sq(ax: f64, az: f64, bx: f64, bz: f64) -> f64 {
    let (dx, dz) = (ax - bx, az - bz);
    dx * dx + dz * dz
}

fn dist(ax: f64, az: f64, bx: f64, bz: f64) -> f64 {
    dist_sq(ax, az, bx, bz).sqrt()
}

/// First-order lerp toward `target` with time constant `tau` seconds
/// (three.js `MathUtils.damp`).
fn damp(current: f64, target: f64, tau: f64, dt_sec: f64) -> f64 {
    current + (target - current) * (1.0 - (-dt_sec / tau).exp())
}

/// Records the event only when the local player is one end of the pair.
fn emit(events: &mut Vec<ProximityEvent>, self_id: &str, key: &str, kind: ProximityEventKind) {
    let (a, b) = split_key(key);
    let other = if a == self_id && b != self_id {
        b
    } else if b == self_id && a != self_id {
        a
    } else {
        return;
    };
    events.push(ProximityEvent {
        kind,
        other_id: other.to_owned(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrapolation_freezes_at_the_cap() {
        let pos = Vec3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let vel = Vec3 {
            x: 2.0,
            y: 0.0,
            z: -1.0,
        };
        assert_eq!(extrapolate_pos(pos, vel, None, 500.0), pos);
        let fresh = extrapolate_pos(pos, vel, Some(450.0), 500.0);
        assert!((fresh.x - 1.1).abs() < 1e-12 && (fresh.z + 0.05).abs() < 1e-12);
        let stale = extrapolate_pos(pos, vel, Some(0.0), 500.0);
        assert!((stale.x - 1.2).abs() < 1e-12 && (stale.z + 0.1).abs() < 1e-12);
    }
}
