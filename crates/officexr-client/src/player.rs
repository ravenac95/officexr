//! The local player: keyboard intent stepped through `officexr-sim` each frame, as
//! `SceneFrame.tsx` does, drawn with the peers and broadcast to them.

use bevy::prelude::*;
use officexr_render::{PlayerRender, Players};
use officexr_sim::physics::character_movement::XZ;
use officexr_sim::physics::rules::SPAWN_DROP_HEIGHT;
use officexr_sim::{SimCore, SimInput, SimPeer, Vec3 as SimVec3};

use crate::baked_colliders::BakedColliders;
use crate::broadcaster::{Pose, PositionBroadcaster};
use crate::camera::Orbit;
use crate::net::{Joined, Outbound};
use crate::peers::character_for;
use crate::world::{CHAR_RADIUS, SimWorld};

/// The camera looks at the avatar's chest, not its feet.
const FOLLOW_HEIGHT: f32 = 1.2;

#[derive(Resource)]
pub struct LocalPlayer {
    sim: SimCore,
    broadcaster: PositionBroadcaster,
    /// `None` until the colliders are in and the player has spawned.
    pose: Option<Pose>,
    script: Option<Script>,
}

impl Default for LocalPlayer {
    fn default() -> Self {
        Self {
            sim: SimCore::new(CHAR_RADIUS),
            broadcaster: PositionBroadcaster::default(),
            pose: None,
            script: Script::from_env(),
        }
    }
}

/// (Re)builds the static colliders, spawning the player the first time.
pub fn apply_colliders(
    world: Option<Res<SimWorld>>,
    baked: Option<Res<BakedColliders>>,
    outbound: Res<Outbound>,
    mut player: ResMut<LocalPlayer>,
) {
    let (Some(world), Some(baked)) = (world, baked) else {
        return;
    };
    if !baked.is_changed() {
        return;
    }
    if let Err(err) = player
        .sim
        .set_static_colliders(&world.objects, &world.kinds, &baked.0)
    {
        error!("static colliders: {err:?}");
        return;
    }
    if player.pose.is_none() {
        let spawn = world.spawn_points.first().copied().unwrap_or_default();
        let pos = SimVec3 {
            y: spawn.y + SPAWN_DROP_HEIGHT,
            ..spawn
        };
        player.sim.teleport(pos);
        let pose = Pose {
            pos,
            vel: SimVec3::default(),
            yaw: 0.0,
            is_airborne: false,
        };
        player.pose = Some(pose);
        outbound.send_position(player.broadcaster.announce(pose));
    }
}

// SRP bend, as in SceneFrame: one system reads intent, steps the sim, broadcasts and moves the
// camera target, because each needs this frame's pose in that order.
#[allow(clippy::too_many_arguments)]
pub fn step_player(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    world: Option<Res<SimWorld>>,
    outbound: Res<Outbound>,
    mut joined: MessageReader<Joined>,
    mut player: ResMut<LocalPlayer>,
    mut players: ResMut<Players>,
    mut orbit: ResMut<Orbit>,
) {
    let rejoined = joined.read().count() > 0;
    let (Some(world), Some(pose)) = (world, player.pose) else {
        return;
    };
    let player = &mut *player;
    if rejoined {
        outbound.send_position(player.broadcaster.announce(pose));
    }

    let intent = match player.script.as_mut() {
        Some(script) => script.next(),
        None => Intent::from_keys(&keys),
    };
    let (fwd, strafe) = (f64::from(intent.fwd), f64::from(intent.strafe));
    let yaw = f64::from(orbit.yaw);
    let dx = -yaw.sin() * fwd + yaw.cos() * strafe;
    let dz = -yaw.cos() * fwd - yaw.sin() * strafe;
    let len = dx.hypot(dz);
    let dir = if len > 0.0 {
        XZ {
            x: dx / len,
            z: dz / len,
        }
    } else {
        XZ::default()
    };

    let peers: Vec<SimPeer> = players
        .players
        .iter()
        .map(|p| SimPeer {
            id: p.id.clone(),
            pos: SimVec3 {
                x: f64::from(p.pos.x),
                y: f64::from(p.pos.y),
                z: f64::from(p.pos.z),
            },
        })
        .collect();
    player.sim.sync_peers(&peers, CHAR_RADIUS);

    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let result = player.sim.step(&SimInput {
        dt_sec: time.delta_secs_f64(),
        now_ms,
        dir,
        // ponytail: per-character speed configs and speedScale are ignored; world constants only.
        speed: if intent.run {
            world.player_speed * world.run_multiplier
        } else {
            world.player_speed
        },
        running: intent.run,
        yaw: if len > 0.0 {
            (-dir.x).atan2(-dir.z)
        } else {
            pose.yaw
        },
        jump: intent.jump,
        settings: world.settings.clone(),
        spawn_points: world.spawn_points.clone(),
    });
    let pose = Pose {
        pos: result.new_pos,
        vel: result.broadcast_vel,
        yaw: result.broadcast_yaw,
        is_airborne: result.is_airborne,
    };
    player.pose = Some(pose);
    if let Some(sent) = player.broadcaster.flush(pose, now_ms) {
        outbound.send_position(sent);
    }

    let at = |v: SimVec3| Vec3::new(v.x as f32, v.y as f32, v.z as f32);
    players.player_speed = world.player_speed as f32;
    players.run_speed = (world.player_speed * world.run_multiplier) as f32;
    players.players.push(PlayerRender {
        id: outbound.self_id.clone(),
        character: character_for(&outbound.self_id, None).into(),
        pos: at(pose.pos),
        yaw: pose.yaw as f32,
        vel: at(pose.vel),
        name: String::new(),
        airborne: pose.is_airborne,
    });
    orbit.target = at(pose.pos) + Vec3::Y * FOLLOW_HEIGHT;
}

#[derive(Clone, Copy, Default)]
struct Intent {
    fwd: f32,
    strafe: f32,
    run: bool,
    /// Pressed this frame.
    jump: bool,
}

impl Intent {
    fn from_keys(keys: &ButtonInput<KeyCode>) -> Self {
        let axis = |plus: [KeyCode; 2], minus: [KeyCode; 2]| {
            f32::from(u8::from(keys.any_pressed(plus)))
                - f32::from(u8::from(keys.any_pressed(minus)))
        };
        Self {
            fwd: axis(
                [KeyCode::KeyW, KeyCode::ArrowUp],
                [KeyCode::KeyS, KeyCode::ArrowDown],
            ),
            strafe: axis(
                [KeyCode::KeyD, KeyCode::ArrowRight],
                [KeyCode::KeyA, KeyCode::ArrowLeft],
            ),
            run: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
            jump: keys.just_pressed(KeyCode::Space),
        }
    }
}

/// Test-only scripted input for headless live checks, from `OFFICEXR_TEST_INPUT`: comma-separated
/// `<keys>*<frames>` steps, `keys` a `+`-joined subset of `w a s d shift space` or `-` for none,
/// e.g. `-*60,w*120,w+shift*60,space*1`. Space jumps on a step's first frame only.
struct Script {
    steps: Vec<(Intent, u32)>,
    step: usize,
    frame: u32,
}

impl Script {
    fn from_env() -> Option<Self> {
        let spec = std::env::var("OFFICEXR_TEST_INPUT").ok()?;
        match Self::parse(&spec) {
            Ok(steps) => Some(Self {
                steps,
                step: 0,
                frame: 0,
            }),
            Err(err) => {
                error!("OFFICEXR_TEST_INPUT: {err}");
                None
            }
        }
    }

    fn parse(spec: &str) -> Result<Vec<(Intent, u32)>, String> {
        spec.split(',')
            .map(|step| {
                let (keys, frames) = step.split_once('*').ok_or(format!("{step}: no *frames"))?;
                let frames = frames.parse().map_err(|_| format!("{step}: bad frames"))?;
                let mut intent = Intent::default();
                for key in keys.split('+') {
                    match key {
                        "w" => intent.fwd += 1.0,
                        "s" => intent.fwd -= 1.0,
                        "d" => intent.strafe += 1.0,
                        "a" => intent.strafe -= 1.0,
                        "shift" => intent.run = true,
                        "space" => intent.jump = true,
                        "-" => {}
                        _ => return Err(format!("{step}: unknown key {key}")),
                    }
                }
                Ok((intent, frames))
            })
            .collect()
    }

    /// Idle once the script runs out.
    fn next(&mut self) -> Intent {
        let Some(&(intent, frames)) = self.steps.get(self.step) else {
            return Intent::default();
        };
        let first = self.frame == 0;
        self.frame += 1;
        if self.frame >= frames {
            (self.step, self.frame) = (self.step + 1, 0);
        }
        Intent {
            jump: intent.jump && first,
            ..intent
        }
    }
}
