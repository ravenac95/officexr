//! Renders a scene offscreen and writes a PNG, then prints the mean frame
//! time.
//!
//! render <scene.json> <out.png> <width>x<height> <px,py,pz> <tx,ty,tz> <fov°> [asset base]
//!
//! The camera is the one `renderFraming.ts`'s `frameCamera` returns; the
//! asset base is as for the `viewer` example.

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::ExitCondition;
use officexr_render::{AssetBase, OfficeXrRenderPlugin, SceneCamera, WorldSceneData};

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 6 {
        eprintln!(
            "usage: render <scene.json> <out.png> <w>x<h> <px,py,pz> <tx,ty,tz> <fov> [asset base]"
        );
        std::process::exit(2);
    }
    let scene: WorldSceneData = serde_json::from_str(
        &std::fs::read_to_string(&args[0]).unwrap_or_else(|e| panic!("{}: {e}", args[0])),
    )
    .unwrap_or_else(|e| panic!("{}: {e}", args[0]));
    let (w, h) = args[2]
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or_else(|| panic!("{}: expected <w>x<h>", args[2]));
    let base = AssetBase::parse(args.get(6).cloned().unwrap_or_else(|| {
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
        })
        .insert_resource(Output {
            image,
            path: args[1].clone(),
        })
        .add_systems(Update, (target_image, capture_and_exit))
        .run();
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

fn capture_and_exit(
    mut commands: Commands,
    mut frame: Local<u32>,
    mut measured: Local<Duration>,
    time: Res<Time<Real>>,
    output: Res<Output>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    if (MEASURE_FROM..CAPTURE_AT).contains(&*frame) {
        *measured += time.delta();
    }
    if *frame == CAPTURE_AT {
        let mean = *measured / (CAPTURE_AT - MEASURE_FROM);
        println!("frame_ms={:.2}", mean.as_secs_f64() * 1e3);
        commands
            .spawn(Screenshot::image(output.image.clone()))
            .observe(save_to_disk(output.path.clone()));
    } else if *frame == EXIT_AT {
        exit.write(AppExit::Success);
    }
}
