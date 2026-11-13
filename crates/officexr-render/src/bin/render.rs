//! Renders a scene offscreen and writes a PNG, then prints the mean frame
//! time.
//!
//! cargo run -p officexr-render --features mugshot --bin render -- [--mugshot <character>] <scene.json> <out.png> <width>x<height> <px,py,pz> <tx,ty,tz> <fov°> <near> <far> [asset base]
//!
//! The camera is the one `renderFraming.ts`'s `frameCamera` returns; the
//! asset base is as for the `viewer` example. `--mugshot` drops the
//! character from `SPAWN_DROP_HEIGHT` above the origin, lets it settle
//! on the scene's colliders under gravity, and draws it paused, as the
//! studio's Mugshot mode does.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::ExitCondition;
use officexr_render::{
    AssetBase, OfficeXrRenderPlugin, PendingKinds, PendingLayouts, PlayerRender, Players,
    SceneCamera, WorldSceneData,
};
use officexr_sim::physics::character_movement::XZ;
use officexr_sim::physics::rules::{BODY_Y, SPAWN_DROP_HEIGHT};
use officexr_sim::{SimCore, SimInput, SimKind, SimKinds, SimSettings, WorldObjects};

// ponytail: fixed frame budgets, not a load signal; too short if models take longer to arrive.
const MEASURE_FROM: u32 = 200;
const CAPTURE_AT: u32 = 300;
const EXIT_AT: u32 = 320;

#[derive(Resource)]
struct Output {
    image: Handle<Image>,
    path: String,
}

fn parse_vec3(s: &str) -> [f32; 3] {
    let v: Vec<f32> = s
        .split(',')
        .map(|n| n.parse().unwrap_or_else(|e| panic!("{s}: {e}")))
        .collect();
    v.try_into()
        .unwrap_or_else(|_| panic!("{s}: expected x,y,z"))
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mugshot = (args.first().map(String::as_str) == Some("--mugshot")).then(|| {
        let character = args.get(1).cloned().expect("--mugshot <character>");
        args.drain(..2);
        character
    });
    if args.len() < 8 {
        eprintln!(
            "usage: render <scene.json> <out.png> <w>x<h> <px,py,pz> <tx,ty,tz> <fov> <near> <far> [asset base]"
        );
        std::process::exit(2);
    }
    let scene_json =
        std::fs::read_to_string(&args[0]).unwrap_or_else(|e| panic!("{}: {e}", args[0]));
    let scene: WorldSceneData =
        serde_json::from_str(&scene_json).unwrap_or_else(|e| panic!("{}: {e}", args[0]));
    let (w, h) = args[2]
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or_else(|| panic!("{}: expected <w>x<h>", args[2]));
    let base = AssetBase::parse(args.get(8).cloned().unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/studio/public").into()
    }));

    let mut app = App::new();
    app.add_plugins((
        base,
        DefaultPlugins.set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        }),
        ScheduleRunnerPlugin::run_loop(Duration::ZERO),
        OfficeXrRenderPlugin,
    ));
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            w,
            h,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    app.insert_resource(scene)
        .insert_resource(SceneCamera {
            position: parse_vec3(&args[3]),
            look_at: parse_vec3(&args[4]),
            fov_deg: args[5].parse().expect("fov"),
            near: args[6].parse().expect("near"),
            far: args[7].parse().expect("far"),
        })
        .insert_resource(Output {
            image,
            path: args[1].clone(),
        })
        .add_systems(Update, (target_image, capture_and_exit));
    if let Some(character) = mugshot {
        app.insert_resource(settled_player(&scene_json, character));
    }
    app.run();
}

/// The scene's kinds and objects, read as the sim's collider inputs.
#[derive(serde::Deserialize)]
struct SimScene {
    kinds: Vec<SimKind>,
    objects: WorldObjects,
}

fn settled_player(scene_json: &str, character: String) -> Players {
    let scene: SimScene = serde_json::from_str(scene_json).expect("scene colliders");
    let players = Players {
        paused: true,
        body_y: BODY_Y as f32,
        ..default()
    };
    let settings = SimSettings {
        char_radius: players.char_radius.into(),
        movement_block_threshold: 0.9,
        jump_velocity: 8.0,
        air_control: 0.2,
        max_jumps: 2,
        landing_ease_ms: 120.0,
    };
    let mut sim = SimCore::new(settings.char_radius);
    let kinds = SimKinds {
        voxel_size: scene.objects.cube_size,
        kinds: scene.kinds,
    };
    sim.set_static_colliders(&scene.objects, &kinds, &[])
        .expect("valid colliders");
    sim.teleport(officexr_sim::Vec3 {
        x: 0.0,
        y: SPAWN_DROP_HEIGHT,
        z: 0.0,
    });
    let input = SimInput {
        dt_sec: 1.0 / 60.0,
        now_ms: 0.0,
        dir: XZ { x: 0.0, z: 0.0 },
        speed: 0.0,
        running: false,
        yaw: 0.0,
        jump: false,
        settings,
        spawn_points: vec![],
    };
    let pos = (0..600)
        .map(|_| sim.step(&input))
        .find(|r| r.is_grounded)
        .expect("the character never landed")
        .new_pos;
    println!("settled_y={}", pos.y);
    Players {
        players: vec![PlayerRender {
            id: "mugshot-player".into(),
            character,
            pos: Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
            yaw: 0.0,
            vel: Vec3::ZERO,
            name: String::new(),
            airborne: false,
        }],
        ..players
    }
}

fn target_image(
    mut commands: Commands,
    cameras: Query<Entity, Added<Camera3d>>,
    output: Res<Output>,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(RenderTarget::Image(output.image.clone().into()));
    }
}

#[allow(clippy::too_many_arguments)]
fn capture_and_exit(
    mut commands: Commands,
    mut frame: Local<u32>,
    mut measured: Local<Duration>,
    time: Res<Time<Real>>,
    output: Res<Output>,
    kinds: Res<PendingKinds>,
    layouts: Res<PendingLayouts>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    if (MEASURE_FROM..CAPTURE_AT).contains(&*frame) {
        *measured += time.delta();
    }
    if *frame == CAPTURE_AT {
        let mean = *measured / (CAPTURE_AT - MEASURE_FROM);
        if !kinds.is_empty() || !layouts.is_empty() {
            warn!("capturing with models still loading");
        }
        println!("frame_ms={:.2}", mean.as_secs_f64() * 1e3);
        commands
            .spawn(Screenshot::image(output.image.clone()))
            .observe(save_to_disk(output.path.clone()));
    } else if *frame == EXIT_AT {
        exit.write(AppExit::Success);
    }
}
