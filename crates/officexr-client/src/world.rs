use std::collections::HashSet;

use bevy::prelude::*;
use officexr_render::{
    BackgroundConfig, LightingConfig, WorldObjectKind, WorldObjects, WorldSceneData,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Message)]
pub enum WorldUpdate {
    Kinds(Vec<WorldObjectKind>),
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
    objects: Option<WorldObjects>,
    constants: Option<Constants>,
}

// ponytail: baked layouts are not drawn; their instances render one by one from `world:objects`.
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
        baked_layouts: vec![],
        clear_color: None,
    })
}
