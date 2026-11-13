use std::collections::HashSet;

use bevy::prelude::*;
use officexr_render::{
    BackgroundConfig, BakedLayoutPlacement, LightingConfig, WorldObjectKind, WorldObjects,
    WorldSceneData,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Message)]
pub enum WorldUpdate {
    Kinds(Vec<WorldObjectKind>),
    Layouts(Vec<BakedLayoutPlacement>),
    /// A `WorldObjects` payload.
    Objects(Value),
    /// A `WorldConstants` payload.
    Constants(Value),
}

#[derive(Deserialize)]
struct Constants {
    lighting: LightingConfig,
    background: BackgroundConfig,
}

#[derive(Default)]
pub struct Assembly {
    kinds: Option<Vec<WorldObjectKind>>,
    layouts: Vec<BakedLayoutPlacement>,
    objects: Option<WorldObjects>,
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
            WorldUpdate::Layouts(layouts) => assembly.layouts = layouts.clone(),
            WorldUpdate::Objects(value) => match WorldObjects::deserialize(value) {
                Ok(objects) => assembly.objects = Some(objects),
                Err(err) => warn!("dropping world objects: {err}"),
            },
            WorldUpdate::Constants(value) => match Constants::deserialize(value) {
                Ok(constants) => assembly.constants = Some(constants),
                Err(err) => warn!("dropping world constants: {err}"),
            },
        }
    }
    if changed && let Some(scene) = scene(&assembly) {
        commands.insert_resource(scene);
    }
}

/// The scene once kinds and constants are in; objects default to none until a peer sends them.
fn scene(assembly: &Assembly) -> Option<WorldSceneData> {
    let (kinds, constants) = (assembly.kinds.as_ref()?, assembly.constants.as_ref()?);
    let objects = assembly
        .objects
        .clone()
        .unwrap_or(WorldObjects { instances: vec![] });
    let used: HashSet<_> = objects.instances.iter().map(|i| &i.kind_id).collect();
    Some(WorldSceneData {
        kinds: kinds
            .iter()
            .filter(|k| used.contains(&k.id))
            .cloned()
            .collect(),
        objects,
        lighting: constants.lighting.clone(),
        background: constants.background.clone(),
        baked_layouts: assembly.layouts.clone(),
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

    fn kind(id: &str) -> WorldObjectKind {
        serde_json::from_value(serde_json::json!({
            "id": id, "gltfPath": "/m.glb", "scale": 1, "tint": null, "opacity": 1,
            "roughness": null, "metalness": null, "emissive": null, "emissiveIntensity": 0,
        }))
        .unwrap()
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
        app.world_mut()
            .write_message(WorldUpdate::Layouts(vec![BakedLayoutPlacement {
                gltf_path: "/l.glb".into(),
                position: [0.0; 3],
                rotation_y: 0,
            }]));
        app.update();
        let scene = app.world().resource::<WorldSceneData>();
        let kinds: Vec<_> = scene.kinds.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(kinds, ["k"]);
        assert_eq!(scene.baked_layouts.len(), 1);
    }
}
