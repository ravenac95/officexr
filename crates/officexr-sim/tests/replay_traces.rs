use std::path::Path;

use officexr_sim::trace::{
    ProximityFrameOutput, ProximityTrace, SimTrace, replay, replay_proximity,
};
use officexr_sim::{SimCore, SimStepResult, Vec3};

const TOLERANCE: f64 = 1e-4;
const TRACE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/world/src/sim/traces"
);

fn max_diff(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x)
        .abs()
        .max((a.y - b.y).abs())
        .max((a.z - b.z).abs())
}

/// Largest position and velocity errors, after asserting every exact field.
fn compare(name: &str, actual: &[SimStepResult], golden: &[SimStepResult]) -> (f64, f64) {
    assert_eq!(actual.len(), golden.len(), "{name}: frame count");
    let (mut pos, mut vel) = (0.0_f64, 0.0_f64);
    for (i, (a, g)) in actual.iter().zip(golden).enumerate() {
        let at = format!("{name}[{i}]");
        assert_eq!(a.moved, g.moved, "{at}.moved");
        assert_eq!(a.anim_state, g.anim_state, "{at}.animState");
        assert_eq!(a.is_grounded, g.is_grounded, "{at}.isGrounded");
        assert_eq!(
            a.has_floor_underneath, g.has_floor_underneath,
            "{at}.hasFloorUnderneath"
        );
        assert_eq!(a.respawned, g.respawned, "{at}.respawned");
        assert_eq!(a.is_airborne, g.is_airborne, "{at}.isAirborne");
        assert_eq!(a.broadcast_yaw, g.broadcast_yaw, "{at}.broadcastYaw");
        let ids = |r: &SimStepResult| {
            r.bumps
                .iter()
                .map(|b| b.other_id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(a), ids(g), "{at}.bumps");
        for (ab, gb) in a.bumps.iter().zip(&g.bumps) {
            let n = (ab.normal.x - gb.normal.x)
                .abs()
                .max((ab.normal.z - gb.normal.z).abs());
            assert!(n <= TOLERANCE, "{at}.bumps normal off by {n}");
        }
        let frame_pos = max_diff(a.new_pos, g.new_pos);
        let frame_vel = max_diff(a.broadcast_vel, g.broadcast_vel).max((a.vel_y - g.vel_y).abs());
        pos = pos.max(frame_pos);
        vel = vel.max(frame_vel);
        assert!(
            frame_pos <= TOLERANCE,
            "{at}.newPos {:?} vs {:?}",
            a.new_pos,
            g.new_pos
        );
        assert!(
            frame_vel <= TOLERANCE,
            "{at} velocity {:?}/{} vs {:?}/{}",
            a.broadcast_vel,
            a.vel_y,
            g.broadcast_vel,
            g.vel_y
        );
    }
    (pos, vel)
}

/// Largest centre/radius/focus error, after asserting every exact field.
fn compare_proximity(
    name: &str,
    actual: &[ProximityFrameOutput],
    golden: &[ProximityFrameOutput],
) -> f64 {
    assert_eq!(actual.len(), golden.len(), "{name}: frame count");
    let mut worst = 0.0_f64;
    for (i, (a, g)) in actual.iter().zip(golden).enumerate() {
        let at = format!("{name}[{i}]");
        assert_eq!(a.events, g.events, "{at}.events");
        assert_eq!(a.pair_states, g.pair_states, "{at}.pairStates");
        assert_eq!(a.player_states, g.player_states, "{at}.playerStates");
        assert_eq!(a.areas.len(), g.areas.len(), "{at}.areas");
        for (aa, ga) in a.areas.iter().zip(&g.areas) {
            assert_eq!((&aa.id, &aa.members), (&ga.id, &ga.members), "{at}.areas");
            worst = worst
                .max((aa.center.x - ga.center.x).abs())
                .max((aa.center.z - ga.center.z).abs())
                .max((aa.radius - ga.radius).abs());
        }
        match (a.conversation_focus, g.conversation_focus) {
            (Some(af), Some(gf)) => {
                worst = worst.max((af.x - gf.x).abs()).max((af.z - gf.z).abs());
            }
            (af, gf) => assert_eq!(af, gf, "{at}.conversationFocus"),
        }
        assert!(worst <= TOLERANCE, "{at} geometry off by {worst}");
    }
    worst
}

#[test]
fn replays_every_trace() {
    let mut paths: Vec<_> = std::fs::read_dir(Path::new(TRACE_DIR))
        .expect("trace dir")
        .map(|e| e.expect("trace entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no traces in {TRACE_DIR}");
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        if json["kind"] == "proximity" {
            let trace: ProximityTrace =
                serde_json::from_value(json).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut sim = SimCore::new(trace.inputs[0].input.settings.char_radius);
            let outputs = replay_proximity(&mut sim, &trace.inputs);
            let err = compare_proximity(&name, &outputs, &trace.outputs);
            println!(
                "{name}: {} frames exact, max geometry error {err:e} m",
                outputs.len()
            );
            continue;
        }
        let trace: SimTrace =
            serde_json::from_value(json).unwrap_or_else(|e| panic!("{name}: {e}"));
        let char_radius = trace.inputs[0].step.settings.char_radius;
        let mut sim = SimCore::new(char_radius);
        let outputs = replay(&mut sim, &trace.map, &trace.inputs).expect("valid colliders");
        let (pos, vel) = compare(&name, &outputs, &trace.outputs);
        println!("{name}: max position error {pos:e} m, max velocity error {vel:e} m/s");
    }
}
