//! Renders each `scenarios/<name>.json` headless, asserts the sim's
//! character positions, and compares its keyframes with
//! `baselines/<name>/`.
//!
//! cargo test -p officexr-render --features visual-tests [<name filter>...]
//!
//! Needs a Vulkan adapter (Mesa lavapipe will do) and, for the scenario
//! maps' prototype models, `pnpm asset-packs:install`. Each scenario writes
//! its keyframes and `video.mp4` (a `video/` PNG sequence without ffmpeg on
//! PATH) to `target/tmp/visual-tests/<name>/`. A keyframe with no baseline
//! fails; `OFFICEXR_UPDATE_BASELINES=1` writes them all from a run whose
//! other checks pass. Review new baselines by eye: never widen the
//! threshold to pass.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::winit::WinitPlugin;
use officexr_render::placement::VOXEL_SIZE;
use officexr_render::{
    AssetBase, AvatarBound, LayoutIndex, ObjectKinds, OffscreenTarget, PlayerRender, Players,
    SceneCamera, WorldSceneData, offscreen_app, scene_sim, sim_input,
};
use officexr_sim::SimPeer;
use officexr_sim::physics::character_movement::XZ;
use serde::Deserialize;
use serde_json::{Value, json};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const HERE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/visual");
/// `tests/playwright/CLAUDE.md`'s gravity contract.
const MAX_GROUND_SINK: f32 = 0.10;
const PIXEL_THRESHOLD: u32 = 6;
const MAX_DIFF_FRAC: f64 = 0.01;
/// Every other 60 Hz sim frame makes a 30 fps video.
const VIDEO_STRIDE: usize = 2;
const READY_FRAMES: u32 = 10;
const LOAD_TIMEOUT_FRAMES: u32 = 3000;
const SAVE_FRAMES: u32 = 30;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Scenario {
    size: [u32; 2],
    /// Bind pose, as the Mugshot draws.
    #[serde(default)]
    paused: bool,
    /// A `packages/world/layouts/<name>.json` placed at the origin.
    layout: Option<String>,
    #[serde(default)]
    instances: Vec<Instance>,
    characters: Vec<Character>,
    cameras: Vec<CameraPose>,
    /// Sim frames to save from every camera.
    captures: Vec<usize>,
    assert: Vec<Assertion>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Instance {
    kind_id: String,
    /// Voxels.
    position: [f64; 3],
    #[serde(default)]
    yaw_deg: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutCommand {
    op: String,
    #[serde(flatten)]
    instance: Instance,
}

#[derive(Deserialize)]
struct Layout {
    commands: Vec<LayoutCommand>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Character {
    character: String,
    /// Body root, teleported to before the first step.
    spawn: [f64; 3],
    moves: Vec<Move>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    frames: usize,
    /// XZ walk direction; absent stands still.
    #[serde(default)]
    dir: [f64; 2],
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CameraPose {
    name: String,
    position: [f32; 3],
    look_at: [f32; 3],
    fov: f32,
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Assertion {
    /// Every character's feet within `MAX_GROUND_SINK` of `surface_y`.
    FeetOn { frame: usize, surface_y: f32 },
    /// The XZ distance between the first two characters.
    Spacing { frame: usize, min: f32, max: f32 },
    /// A character's body root within `tolerance` of `position`.
    Near {
        frame: usize,
        character: usize,
        position: [f32; 3],
        tolerance: f32,
    },
}

/// One rendered update: the sim frame on show, through `camera`, saved to `files`.
struct Step {
    frame: usize,
    camera: usize,
    files: Vec<PathBuf>,
}

#[derive(Resource)]
struct Timeline {
    frames: Vec<Vec<PlayerRender>>,
    steps: Vec<Step>,
    cameras: Vec<CameraPose>,
    next: usize,
    ready_for: u32,
    loading_for: u32,
    done_for: u32,
    /// The models that failed to load, for after `App::run` takes the world.
    failed: Arc<Mutex<Vec<String>>>,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `WorldSceneData` JSON: the scenario's instances with their catalog kinds.
fn scene_json(scenario: &Scenario) -> String {
    let layout = scenario.layout.iter().flat_map(|name| {
        let layout: Layout =
            read_json(&Path::new(ROOT).join(format!("packages/world/layouts/{name}.json")));
        layout.commands.into_iter().map(move |c| {
            assert_eq!(c.op, "placeObject", "{name}: unsupported layout op");
            c.instance
        })
    });
    let instances: Vec<Value> = layout
        .chain(scenario.instances.iter().cloned())
        .enumerate()
        .map(|(n, i)| {
            json!({ "id": format!("i{n}"), "kindId": i.kind_id, "position": i.position, "yawDeg": i.yaw_deg })
        })
        .collect();
    let used: HashSet<&str> = instances
        .iter()
        .map(|i| i["kindId"].as_str().unwrap())
        .collect();
    let catalog: Value = read_json(&Path::new(ROOT).join("packages/world/world-object-kinds.json"));
    let kinds: Vec<&Value> = catalog["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|k| used.contains(k["id"].as_str().unwrap()))
        .collect();
    let lighting: Value = read_json(&Path::new(HERE).join("lighting.json"));
    json!({
        "kinds": kinds,
        "objects": { "cubeSize": VOXEL_SIZE, "instances": instances },
        "lighting": lighting["lighting"],
        "background": lighting["background"],
    })
    .to_string()
}

/// Every character's state after each 60 Hz step, each sim seeing the
/// others where they stood the step before.
fn simulate(scenario: &Scenario, scene_json: &str, players: &Players) -> Vec<Vec<PlayerRender>> {
    let mut sims: Vec<_> = scenario
        .characters
        .iter()
        .map(|c| {
            let mut sim = scene_sim(scene_json, players.char_radius);
            let [x, y, z] = c.spawn;
            sim.teleport(officexr_sim::Vec3 { x, y, z });
            sim
        })
        .collect();
    let dirs: Vec<Vec<[f64; 2]>> = scenario
        .characters
        .iter()
        .map(|c| {
            c.moves
                .iter()
                .flat_map(|m| std::iter::repeat_n(m.dir, m.frames))
                .collect()
        })
        .collect();
    let frame_count = dirs.iter().map(Vec::len).max().unwrap_or(0);
    let mut yaws = vec![0.0f32; sims.len()];
    let mut frames: Vec<Vec<PlayerRender>> = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let peers: Vec<SimPeer> = sims
            .iter()
            .enumerate()
            .map(|(i, s)| SimPeer {
                id: format!("p{i}"),
                pos: s.position(),
            })
            .collect();
        let frame = sims
            .iter_mut()
            .enumerate()
            .map(|(i, sim)| {
                let mut others = peers.clone();
                others.remove(i);
                sim.sync_peers(&others, players.char_radius.into());
                let [dx, dz] = dirs[i].get(f).copied().unwrap_or_default();
                let len = dx.hypot(dz);
                let dir = if len > 0.0 {
                    yaws[i] = (-dx).atan2(-dz) as f32;
                    XZ {
                        x: dx / len,
                        z: dz / len,
                    }
                } else {
                    XZ { x: 0.0, z: 0.0 }
                };
                let r = sim.step(&sim_input(players, dir, players.player_speed, yaws[i]));
                let v = |p: officexr_sim::Vec3| Vec3::new(p.x as f32, p.y as f32, p.z as f32);
                PlayerRender {
                    id: format!("p{i}"),
                    character: scenario.characters[i].character.clone(),
                    pos: v(r.new_pos),
                    yaw: yaws[i],
                    vel: v(r.broadcast_vel),
                    name: String::new(),
                    airborne: r.is_airborne,
                }
            })
            .collect();
        frames.push(frame);
    }
    frames
}

fn check_positions(
    scenario: &Scenario,
    frames: &[Vec<PlayerRender>],
    players: &Players,
) -> Vec<String> {
    let at = |frame: usize| {
        frames
            .get(frame)
            .unwrap_or_else(|| panic!("frame {frame} is past the last move"))
    };
    let mut failures = vec![];
    for a in &scenario.assert {
        match a {
            Assertion::FeetOn { frame, surface_y } => {
                for p in at(*frame) {
                    let feet = p.pos.y + players.body_y - players.char_radius;
                    if (feet - surface_y).abs() > MAX_GROUND_SINK {
                        failures.push(format!(
                            "frame {frame}: {}'s feet at y={feet:.3}, not on the surface at y={surface_y}",
                            p.character
                        ));
                    }
                }
            }
            Assertion::Spacing { frame, min, max } => {
                let [a, b] = [&at(*frame)[0], &at(*frame)[1]];
                let d = a.pos.xz().distance(b.pos.xz());
                if !(*min..=*max).contains(&d) {
                    failures.push(format!(
                        "frame {frame}: {} and {} are {d:.3} m apart, outside [{min}, {max}]",
                        a.character, b.character
                    ));
                }
            }
            Assertion::Near {
                frame,
                character,
                position,
                tolerance,
            } => {
                let p = &at(*frame)[*character];
                let d = p.pos.distance(Vec3::from(*position));
                if d > *tolerance {
                    failures.push(format!(
                        "frame {frame}: {} at {:.3}, {d:.3} m from {position:?}",
                        p.character, p.pos
                    ));
                }
            }
        }
    }
    failures
}

/// One update per sim frame, through the first camera, then one more per
/// further camera on a capture frame.
fn plan(scenario: &Scenario, frame_count: usize, out: &Path) -> (Vec<Step>, Vec<PathBuf>) {
    if let Some(late) = scenario.captures.iter().find(|&&f| f >= frame_count) {
        panic!("capture frame {late} is past the last move");
    }
    let mut steps = vec![];
    let mut keyframes = vec![];
    for frame in 0..frame_count {
        let captured = scenario.captures.contains(&frame);
        for (camera, pose) in scenario.cameras.iter().enumerate() {
            if camera > 0 && !captured {
                break;
            }
            let mut files = vec![];
            if camera == 0 && frame % VIDEO_STRIDE == 0 {
                files.push(out.join(format!("video/{:04}.png", frame / VIDEO_STRIDE)));
            }
            if captured {
                let file = out.join(format!("{frame:04}-{}.png", pose.name));
                keyframes.push(file.clone());
                files.push(file);
            }
            steps.push(Step {
                frame,
                camera,
                files,
            });
        }
    }
    (steps, keyframes)
}

fn scene_camera(pose: &CameraPose) -> SceneCamera {
    SceneCamera {
        position: pose.position,
        look_at: pose.look_at,
        fov_deg: pose.fov,
        near: 0.1,
        far: 2000.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    mut timeline: ResMut<Timeline>,
    mut players: ResMut<Players>,
    mut camera: ResMut<SceneCamera>,
    views: Query<(&ObjectKinds, &LayoutIndex)>,
    bound: Query<(), With<AvatarBound>>,
    target: Res<OffscreenTarget>,
    mut time_step: ResMut<TimeUpdateStrategy>,
    mut exit: MessageWriter<AppExit>,
) {
    let t = &mut *timeline;
    if t.ready_for < READY_FRAMES {
        let ready = !views.is_empty()
            && views.iter().all(|(k, l)| k.pending() + l.pending() == 0)
            && bound.iter().count() == players.players.len();
        if ready {
            t.ready_for += 1;
            // Time stands still while loading, so every clip starts at 0 on the first step.
            if t.ready_for == READY_FRAMES {
                *time_step =
                    TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0));
            }
        } else {
            t.loading_for += 1;
            assert!(
                t.loading_for < LOAD_TIMEOUT_FRAMES,
                "models still loading after {LOAD_TIMEOUT_FRAMES} frames"
            );
        }
        return;
    }
    let Some(step) = t.steps.get(t.next) else {
        t.done_for += 1;
        if t.done_for == SAVE_FRAMES {
            *t.failed.lock().unwrap() = views
                .iter()
                .flat_map(|(kinds, layouts)| {
                    let kinds = kinds.failed().map(|id| format!("kind {id}"));
                    kinds.chain(layouts.failed().map(|path| format!("layout {path}")))
                })
                .collect();
            exit.write(AppExit::Success);
        }
        return;
    };
    players.players.clone_from(&t.frames[step.frame]);
    *camera = scene_camera(&t.cameras[step.camera]);
    if !step.files.is_empty() {
        let mut shot = commands.spawn(Screenshot::image(target.0.clone()));
        for file in &step.files {
            shot.observe(save_to_disk(file.clone()));
        }
    }
    t.next += 1;
}

/// Renders `steps`; fails for each model that didn't load.
fn render(
    scenario: &Scenario,
    scene_json: &str,
    frames: Vec<Vec<PlayerRender>>,
    players: Players,
    steps: Vec<Step>,
) -> Vec<String> {
    let scene: WorldSceneData = serde_json::from_str(scene_json).expect("scene JSON");
    let base = AssetBase::parse(format!("{ROOT}/packages/studio/public"));
    // Winit is in `DefaultPlugins` here only through the viewer example's dev-dependency.
    let plugins = DefaultPlugins
        .build()
        .disable::<WinitPlugin>()
        // An async compile skips draws for however long it takes, which varies by run.
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        });
    let [w, h] = scenario.size;
    let failed: Arc<Mutex<Vec<String>>> = Arc::default();
    let mut app = offscreen_app(base, plugins, w, h);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .insert_resource(scene)
        .insert_resource(scene_camera(&scenario.cameras[0]))
        .insert_resource(Players {
            players: frames[0].clone(),
            ..players
        })
        .insert_resource(Timeline {
            frames,
            steps,
            cameras: scenario.cameras.clone(),
            next: 0,
            ready_for: 0,
            loading_for: 0,
            done_for: 0,
            failed: failed.clone(),
        })
        .add_systems(PreUpdate, drive);
    app.run();
    let failed = failed.lock().unwrap();
    failed
        .iter()
        .map(|what| format!("{what} failed to load: run `pnpm asset-packs:install`"))
        .collect()
}

fn rgba(path: &Path) -> (UVec2, Vec<u8>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let image = Image::from_buffer(
        &bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD,
    )
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let size = image.size();
    let data = image
        .try_into_dynamic()
        .expect("8-bit PNG")
        .to_rgba8()
        .into_raw();
    (size, data)
}

/// Fails when more than `MAX_DIFF_FRAC` of the pixels differ by more than
/// `PIXEL_THRESHOLD`, or the baseline is missing; `update` takes the keyframes.
fn check_pixels(name: &str, keyframes: &[PathBuf], update: bool) -> Vec<String> {
    let mut failures = vec![];
    for keyframe in keyframes {
        let baseline = Path::new(HERE)
            .join("baselines")
            .join(name)
            .join(keyframe.file_name().unwrap());
        if update {
            std::fs::create_dir_all(baseline.parent().unwrap()).unwrap();
            std::fs::copy(keyframe, &baseline).unwrap();
            println!("  new baseline {}: review it by eye", baseline.display());
            continue;
        }
        if !baseline.exists() {
            failures.push(format!(
                "no baseline {}: review the candidate {} and commit it there, or rerun with OFFICEXR_UPDATE_BASELINES=1",
                baseline.display(),
                keyframe.display()
            ));
            continue;
        }
        let (want_size, want) = rgba(&baseline);
        let (got_size, got) = rgba(keyframe);
        if want_size != got_size {
            failures.push(format!(
                "{}: {got_size} vs baseline {want_size}",
                keyframe.display()
            ));
            continue;
        }
        let (want, got) = (want.as_chunks::<4>().0, got.as_chunks::<4>().0);
        let differing = want
            .iter()
            .zip(got)
            .filter(|(a, b)| {
                (0..3).map(|i| u32::from(a[i].abs_diff(b[i]))).sum::<u32>() > PIXEL_THRESHOLD
            })
            .count();
        let frac = differing as f64 / want.len() as f64;
        if frac > MAX_DIFF_FRAC {
            failures.push(format!(
                "{}: {:.3}% of pixels differ from {}",
                keyframe.display(),
                frac * 100.0,
                baseline.display()
            ));
        }
    }
    failures
}

/// `video.mp4` from the `video/` frames when ffmpeg is on PATH.
fn encode_video(out: &Path) {
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-framerate", "30", "-i"])
        .arg(out.join("video/%04d.png"))
        .args([
            "-pix_fmt",
            "yuv420p",
            "-vf",
            "scale=trunc(iw/2)*2:trunc(ih/2)*2",
        ])
        .arg(out.join("video.mp4"))
        .status();
    match status {
        Ok(s) if s.success() => {
            std::fs::remove_dir_all(out.join("video")).unwrap();
            println!("  video {}", out.join("video.mp4").display());
        }
        Ok(s) => println!("  ffmpeg exited {s}; the video stays a PNG sequence"),
        Err(_) => println!("  no ffmpeg; the video stays a PNG sequence"),
    }
}

fn run(name: &str, update: bool) -> Vec<String> {
    // ponytail(OSO-7958): stairs/slope walk stops on the top step; step-up hop overshoots ~0.45 m/frame
    let scenario: Scenario = read_json(&Path::new(HERE).join(format!("scenarios/{name}.json")));
    let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("visual-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(out.join("video")).unwrap();

    let scene = scene_json(&scenario);
    let players = Players {
        paused: scenario.paused,
        ..default()
    };
    let frames = simulate(&scenario, &scene, &players);
    let mut failures = check_positions(&scenario, &frames, &players);
    let (steps, keyframes) = plan(&scenario, frames.len(), &out);
    failures.extend(render(&scenario, &scene, frames, players, steps));
    let missing: Vec<_> = keyframes.iter().filter(|k| !k.exists()).collect();
    assert!(missing.is_empty(), "never saved: {missing:?}");
    encode_video(&out);
    // A failing run's keyframes don't become baselines.
    if failures.is_empty() || !update {
        failures.extend(check_pixels(name, &keyframes, update));
    }
    failures
}

/// Name filters, as libtest takes them: positional arguments, less the
/// values of the flags that take one.
fn filters() -> Vec<String> {
    const VALUED: [&str; 6] = [
        "--test-threads",
        "--skip",
        "--format",
        "--color",
        "--logfile",
        "-Z",
    ];
    let mut args = std::env::args().skip(1);
    let mut filters = vec![];
    while let Some(arg) = args.next() {
        if VALUED.contains(&arg.as_str()) {
            args.next();
        } else if !arg.starts_with('-') {
            filters.push(arg);
        }
    }
    filters
}

fn main() {
    let filters = filters();
    let update = std::env::var_os("OFFICEXR_UPDATE_BASELINES").is_some_and(|v| v == "1");
    let mut names: Vec<String> = std::fs::read_dir(Path::new(HERE).join("scenarios"))
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            path.file_stem().unwrap().to_string_lossy().into_owned()
        })
        .filter(|n| filters.is_empty() || filters.iter().any(|f| n.contains(f.as_str())))
        .collect();
    names.sort();
    if names.is_empty() {
        eprintln!("no scenario matches {filters:?}");
        std::process::exit(1);
    }
    let mut failed = vec![];
    for name in &names {
        println!("scenario {name}");
        let failures = run(name, update);
        for f in &failures {
            println!("  FAIL {f}");
        }
        if !failures.is_empty() {
            failed.push(name.clone());
        }
    }
    println!(
        "{} scenarios, {} failed {failed:?}",
        names.len(),
        failed.len()
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}
