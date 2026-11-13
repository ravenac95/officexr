use std::collections::HashSet;

use bevy::prelude::*;
use officexr_render::placement::VOXEL_SIZE;
use officexr_render::{
    BackgroundConfig, BakedLayoutPlacement, LightingConfig, WorldObjectKind, WorldObjects,
    WorldSceneData,
};
use officexr_sim::{SimKind, SimKinds, SimSettings, Vec3 as SimVec3};
use serde::Deserialize;
use serde_json::Value;

/// A map room's baked layout.
#[derive(Clone, Debug)]
pub struct Layout {
    pub room_instance_id: String,
    pub placement: BakedLayoutPlacement,
}

#[derive(Message)]
pub enum WorldUpdate {
    /// `/api/world-object-kinds` entries.
    Kinds(Vec<Value>),
    Map {
        layouts: Vec<Layout>,
        spawn_points: Vec<SimVec3>,
    },
    /// A `WorldObjects` payload.
    Objects(Value),
    /// A `WorldConstants` payload.
    Constants(Value),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Constants {
    lighting: LightingConfig,
    background: BackgroundConfig,
    animation: Animation,
    jump: Jump,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Animation {
    player_speed: f64,
    run_multiplier: f64,
    movement_block_threshold: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Jump {
    jump_velocity: f64,
    air_control: f64,
    max_jumps: u32,
    landing_ease_ms: f64,
}

/// As `COLLISION_DEFAULTS.charRadius`: not part of the world constants.
pub const CHAR_RADIUS: f64 = 0.4;

/// What the local player's sim needs from the world.
#[derive(Resource)]
pub struct SimWorld {
    pub objects: officexr_sim::WorldObjects,
    pub kinds: SimKinds,
    pub layouts: Vec<Layout>,
    pub spawn_points: Vec<SimVec3>,
    pub settings: SimSettings,
    pub player_speed: f64,
    pub run_multiplier: f64,
}

#[derive(Default)]
pub struct Assembly {
    kinds: Option<Vec<Value>>,
    layouts: Vec<Layout>,
    spawn_points: Vec<SimVec3>,
    objects: Option<Value>,
    constants: Option<Constants>,
}

pub fn assemble_world(
    mut commands: Commands,
    mut updates: MessageReader<WorldUpdate>,
    mut assembly: Local<Assembly>,
) {
    let mut changed = false;
    for update in updates.read() {
        changed = true;
        match update {
            WorldUpdate::Kinds(kinds) => assembly.kinds = Some(kinds.clone()),
            WorldUpdate::Map {
                layouts,
                spawn_points,
            } => {
                assembly.layouts = layouts.clone();
                assembly.spawn_points = spawn_points.clone();
            }
            WorldUpdate::Objects(value) => {
                match (
                    WorldObjects::deserialize(value),
                    officexr_sim::WorldObjects::deserialize(value),
                ) {
                    (Ok(_), Ok(_)) => assembly.objects = Some(value.clone()),
                    (Err(err), _) => warn!("dropping world objects: {err}"),
                    (_, Err(err)) => warn!("dropping world objects: {err}"),
                }
            }
            WorldUpdate::Constants(value) => match Constants::deserialize(value) {
                Ok(constants) => assembly.constants = Some(constants),
                Err(err) => warn!("dropping world constants: {err}"),
            },
        }
    }
    if changed && let (Some(scene), Some(sim)) = (scene(&assembly), sim_world(&assembly)) {
        commands.insert_resource(scene);
        commands.insert_resource(sim);
    }
}

fn objects<T: serde::de::DeserializeOwned>(assembly: &Assembly) -> Option<T> {
    T::deserialize(assembly.objects.as_ref()?).ok()
}

fn sim_world(assembly: &Assembly) -> Option<SimWorld> {
    let (kinds, c) = (assembly.kinds.as_ref()?, assembly.constants.as_ref()?);
    Some(SimWorld {
        objects: objects(assembly).unwrap_or(officexr_sim::WorldObjects {
            cube_size: f64::from(VOXEL_SIZE),
            instances: vec![],
        }),
        kinds: SimKinds {
            voxel_size: f64::from(VOXEL_SIZE),
            kinds: kinds
                .iter()
                .filter_map(|k| SimKind::deserialize(k).ok())
                .collect(),
        },
        layouts: assembly.layouts.clone(),
        spawn_points: assembly.spawn_points.clone(),
        settings: SimSettings {
            char_radius: CHAR_RADIUS,
            movement_block_threshold: c.animation.movement_block_threshold,
            jump_velocity: c.jump.jump_velocity,
            air_control: c.jump.air_control,
            max_jumps: c.jump.max_jumps,
            landing_ease_ms: c.jump.landing_ease_ms,
        },
        player_speed: c.animation.player_speed,
        run_multiplier: c.animation.run_multiplier,
    })
}

/// The scene once kinds and constants are in; objects default to none until a peer sends them.
fn scene(assembly: &Assembly) -> Option<WorldSceneData> {
    let (kinds, constants) = (assembly.kinds.as_ref()?, assembly.constants.as_ref()?);
    let kinds: Vec<WorldObjectKind> = kinds
        .iter()
        .filter_map(|k| match WorldObjectKind::deserialize(k) {
            Ok(kind) => Some(kind),
            Err(err) => {
                warn!("dropping a world object kind: {err}");
                None
            }
        })
        .collect();
    let objects = objects(assembly).unwrap_or(WorldObjects { instances: vec![] });
    let used: HashSet<_> = objects.instances.iter().map(|i| &i.kind_id).collect();
    Some(WorldSceneData {
        kinds: kinds.into_iter().filter(|k| used.contains(&k.id)).collect(),
        objects,
        lighting: constants.lighting.clone(),
        background: constants.background.clone(),
        baked_layouts: assembly
            .layouts
            .iter()
            .map(|l| l.placement.clone())
            .collect(),
        clear_color: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, field: &str) -> Value {
        let path = format!(
            "{}/../officexr-protocol/tests/fixtures/event-{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut event: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        event[field].take()
    }

    fn kind(id: &str) -> Value {
        serde_json::json!({
            "id": id, "gltfPath": "/m.glb", "scale": 1, "tint": null, "opacity": 1,
            "roughness": null, "metalness": null, "emissive": null, "emissiveIntensity": 0,
        })
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_message::<WorldUpdate>()
            .add_systems(Update, assemble_world);
        app
    }

    #[test]
    fn inserts_the_scene_once_kinds_and_constants_are_in() {
        let mut app = app();
        app.world_mut()
            .write_message(WorldUpdate::Kinds(vec![kind("k"), kind("unused")]));
        app.world_mut()
            .write_message(WorldUpdate::Objects(fixture("world_objects", "objects")));
        app.update();
        assert!(app.world().get_resource::<WorldSceneData>().is_none());

        app.world_mut()
            .write_message(WorldUpdate::Constants(fixture(
                "world_constants",
                "constants",
            )));
        app.world_mut().write_message(WorldUpdate::Map {
            layouts: vec![Layout {
                room_instance_id: "r".into(),
                placement: BakedLayoutPlacement {
                    gltf_path: "/l.glb".into(),
                    position: [0.0; 3],
                    rotation_y: 0,
                },
            }],
            spawn_points: vec![],
        });
        app.update();
        let scene = app.world().resource::<WorldSceneData>();
        let kinds: Vec<_> = scene.kinds.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(kinds, ["k"]);
        assert_eq!(scene.baked_layouts.len(), 1);
        let sim = app.world().resource::<SimWorld>();
        assert_eq!(sim.objects.instances.len(), 1);
        assert_eq!(sim.settings.max_jumps, 2);
    }
}
