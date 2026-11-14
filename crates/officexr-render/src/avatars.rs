//! Port of `Players.tsx` + `Adventurer.tsx` + `PlayerNameTag.tsx`: one
//! KayKit character per player, animated with the shared `Rig_Medium_*`
//! clips, with a screen-space name tag over its head.

use std::f32::consts::PI;
use std::time::Duration;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::gltf::Gltf;
use bevy::prelude::*;

use crate::AssetBase;
use crate::lighting::{HemisphereFill, ObjectMaterial};
use crate::views::{View, ViewKind, ViewPlayers, ViewScene};

const RIGS: [&str; 4] = [
    "/models/animations/Rig_Medium_General.glb",
    "/models/animations/Rig_Medium_MovementBasic.glb",
    "/models/animations/Rig_Medium_MovementAdvanced.glb",
    "/models/animations/Rig_Medium_CombatRanged.glb",
];
/// The glTF node every KayKit character and rig animates from.
const RIG_ROOT: &str = "Rig_Medium";
const AVATAR_YAW_OFFSET: f32 = PI;
const TAG_HEIGHT_M: f32 = 2.0;
const WALK_THRESHOLD: f32 = 0.05;
const FADE: Duration = Duration::from_millis(200);

/// One player as the renderer needs it, as `OfficeState.players` holds it.
#[derive(Clone, Debug)]
pub struct PlayerRender {
    pub id: String,
    /// A `/models/characters/<character>.glb` name, e.g. `Barbarian`.
    pub character: String,
    /// Body root: the ball collider's bottom sits `BODY_Y - char_radius` above it.
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: Vec3,
    /// Empty hides the tag, as for the local player.
    pub name: String,
    pub airborne: bool,
}

/// The players to draw this frame, with the world tunables their motion
/// tier and placement derive from.
#[derive(Resource, Clone, Debug)]
pub struct Players {
    pub players: Vec<PlayerRender>,
    pub char_radius: f32,
    /// Height of the ball collider's centre above the body root.
    pub body_y: f32,
    pub player_speed: f32,
    pub run_speed: f32,
    /// Bind pose, no clips and no name tags, as the mugshot renders.
    pub paused: bool,
}

impl Players {
    /// Where `player`'s character model stands: the ball collider's bottom.
    pub fn feet(&self, player: &PlayerRender) -> Vec3 {
        player.pos + Vec3::Y * (self.body_y - self.char_radius)
    }
}

impl Default for Players {
    fn default() -> Self {
        Self {
            players: vec![],
            char_radius: 0.4,
            body_y: 0.9,
            player_speed: 3.0,
            run_speed: 6.0,
            paused: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Idle,
    Walking,
    Running,
    Jumping,
}

impl Motion {
    const ALL: [Motion; 4] = [
        Motion::Idle,
        Motion::Walking,
        Motion::Running,
        Motion::Jumping,
    ];

    fn clip(self) -> &'static str {
        match self {
            Motion::Idle => "Idle_A",
            Motion::Walking => "Walking_C",
            Motion::Running => "Running_A",
            Motion::Jumping => "Jump_Full_Long",
        }
    }
}

/// `Players.tsx`'s locomotion tier, with its enter/exit hysteresis.
pub fn next_motion(
    previous: Motion,
    vel: Vec3,
    airborne: bool,
    player_speed: f32,
    run_speed: f32,
) -> Motion {
    if airborne {
        return Motion::Jumping;
    }
    let speed_sq = vel.x * vel.x + vel.z * vel.z;
    let run = (player_speed + run_speed) / 2.0;
    let walk_enter = (WALK_THRESHOLD * 1.5).powi(2);
    let walk_exit = WALK_THRESHOLD.powi(2);
    let run_enter = (run * 1.05).powi(2);
    let run_exit = (run * 0.95).powi(2);
    match previous {
        Motion::Idle | Motion::Jumping if speed_sq >= run_enter => Motion::Running,
        Motion::Idle | Motion::Jumping if speed_sq >= walk_enter => Motion::Walking,
        Motion::Idle | Motion::Jumping => Motion::Idle,
        Motion::Walking if speed_sq >= run_enter => Motion::Running,
        Motion::Walking if speed_sq <= walk_exit => Motion::Idle,
        Motion::Walking => Motion::Walking,
        Motion::Running if speed_sq <= walk_exit => Motion::Idle,
        Motion::Running if speed_sq <= run_exit => Motion::Walking,
        Motion::Running => Motion::Running,
    }
}

/// Clip time scale: speed over the tier's reference speed, clamped to
/// [0, 1] in tenths. Idle and jumping play at authored speed.
fn anim_scale(motion: Motion, vel: Vec3, player_speed: f32, run_speed: f32) -> f32 {
    let denom = match motion {
        Motion::Running => run_speed,
        Motion::Walking => player_speed,
        Motion::Idle | Motion::Jumping => return 1.0,
    };
    let ratio = if denom > 0.0 {
        Vec2::new(vel.x, vel.z).length() / denom
    } else {
        0.0
    };
    (ratio.clamp(0.0, 1.0) * 10.0).round() / 10.0
}

#[derive(Component)]
struct Avatar {
    id: String,
    character: String,
    motion: Motion,
    tag: Entity,
    /// The `Rig_Medium` node, once bound.
    rig: Option<Entity>,
}

/// Set once the character's skeleton is bound to the clips (or, paused,
/// once its materials are swapped).
#[derive(Component)]
pub struct Bound;

#[derive(Component)]
struct NameTag;

#[derive(Resource)]
struct Clips {
    rigs: Vec<Handle<Gltf>>,
    graph: Option<(Handle<AnimationGraph>, [AnimationNodeIndex; 4])>,
}

pub(crate) fn plugin(app: &mut App) {
    // Spawning a glTF scene copies its entities by reflection; without
    // Bevy's `reflect_auto_register` these are the types it needs.
    app.register_type::<Transform>()
        .register_type::<Visibility>()
        .register_type::<Name>()
        .register_type::<ChildOf>()
        .register_type::<Children>()
        .register_type::<Mesh3d>()
        .register_type::<MeshMaterial3d<StandardMaterial>>()
        .register_type::<bevy::mesh::skinning::SkinnedMesh>()
        .register_type::<GlobalTransform>()
        .register_type::<InheritedVisibility>()
        .register_type::<ViewVisibility>()
        .register_type::<bevy::transform::components::TransformTreeChanged>()
        .register_type::<bevy::camera::primitives::Aabb>()
        .register_type::<bevy::camera::visibility::VisibilityClass>()
        .register_type::<bevy::gltf::GltfMaterialName>()
        .register_type::<bevy::gltf::GltfMeshName>()
        .register_type::<bevy::gltf::GltfMaterialExtras>()
        .register_type::<bevy::gltf::GltfMeshExtras>()
        .register_type::<bevy::gltf::GltfExtras>()
        .register_type::<bevy::gltf::GltfSceneName>()
        .register_type::<bevy::gltf::GltfSceneExtras>()
        .register_type::<bevy::camera::visibility::DynamicSkinnedMeshBounds>();
    app.add_systems(Startup, load_rigs)
        .add_systems(
            Update,
            (build_graph, sync_avatars, bind_avatars, animate, place_tags)
                .chain()
                .in_set(crate::views::SceneSystems)
                .after(crate::views::mirror_resources),
        )
        .add_observer(despawn_tag);
}

fn despawn_tag(remove: On<Remove<Avatar>>, avatars: Query<&Avatar>, mut commands: Commands) {
    if let Ok(avatar) = avatars.get(remove.entity) {
        commands.entity(avatar.tag).try_despawn();
    }
}

fn load_rigs(mut commands: Commands, base: Res<AssetBase>, assets: Res<AssetServer>) {
    commands.insert_resource(Clips {
        rigs: RIGS
            .iter()
            .map(|r| assets.load(base.asset_path(r)))
            .collect(),
        graph: None,
    });
}

fn build_graph(
    mut clips: ResMut<Clips>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if clips.graph.is_some()
        || !clips
            .rigs
            .iter()
            .all(|h| assets.is_loaded_with_dependencies(h))
    {
        return;
    }
    let find = |name: &str| {
        // Earlier rigs win on a name collision, as Adventurer.tsx merges them.
        clips
            .rigs
            .iter()
            .find_map(|h| gltfs.get(h)?.named_animations.get(name).cloned())
            .unwrap_or_else(|| panic!("no {name} clip in the Rig_Medium GLBs"))
    };
    let (graph, nodes) = AnimationGraph::from_clips(Motion::ALL.map(|m| find(m.clip())));
    clips.graph = Some((graphs.add(graph), nodes.try_into().unwrap()));
}

fn sync_avatars(
    mut commands: Commands,
    views: Query<(Entity, &View, &ViewPlayers), Changed<ViewPlayers>>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    mut avatars: Query<(Entity, &Avatar, &mut Transform, &ChildOf)>,
) {
    for (entity, view, players) in &views {
        let view = (entity, view.camera);
        sync_view_avatars(&mut commands, view, players, &base, &assets, &mut avatars);
    }
}

fn sync_view_avatars(
    commands: &mut Commands,
    (view, camera): (Entity, Option<Entity>),
    players: &Players,
    base: &AssetBase,
    assets: &AssetServer,
    avatars: &mut Query<(Entity, &Avatar, &mut Transform, &ChildOf)>,
) {
    for (entity, avatar, _, _) in avatars.iter().filter(|(.., v)| v.parent() == view) {
        let kept = players
            .players
            .iter()
            .any(|p| p.id == avatar.id && p.character == avatar.character);
        if !kept {
            commands.entity(entity).despawn();
        }
    }
    for p in &players.players {
        // ponytail: the yaw snaps; Players.tsx eases it at `turnSpeed`.
        let transform = Transform::from_translation(players.feet(p))
            .with_rotation(Quat::from_rotation_y(p.yaw + AVATAR_YAW_OFFSET));
        let existing = avatars
            .iter_mut()
            .find(|(_, a, _, v)| v.parent() == view && a.id == p.id && a.character == p.character);
        if let Some((_, _, mut t, _)) = existing {
            *t = transform;
            continue;
        }
        let mut tag = commands.spawn((
            NameTag,
            Text::new(p.name.clone()),
            TextFont::from_font_size(14.0),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            Visibility::Hidden,
        ));
        if let Some(camera) = camera {
            tag.insert(UiTargetCamera(camera));
        }
        let tag = tag.id();
        let path = base.asset_path(&format!("/models/characters/{}.glb", p.character));
        // ponytail: no bind-pose foot anchor; every KayKit character's lowest vertex is within 2 mm of y=0.
        commands.spawn((
            Avatar {
                id: p.id.clone(),
                character: p.character.clone(),
                motion: Motion::Idle,
                tag,
                rig: None,
            },
            transform,
            Visibility::default(),
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
            ChildOf(view),
        ));
    }
}

/// The character GLBs carry no animations, so Bevy's loader gives their
/// bones no animation targets. The clips target the rig GLBs' bones by
/// name path from `Rig_Medium`, which the characters share, so tagging
/// each character bone with the same path binds the clips to it.
#[allow(clippy::too_many_arguments)]
fn bind_avatars(
    mut commands: Commands,
    mut avatars: Query<(Entity, &mut Avatar, &ChildOf), Without<Bound>>,
    views: Query<(&ViewPlayers, &ViewScene)>,
    clips: Res<Clips>,
    children: Query<&Children>,
    names: Query<&Name>,
    standard: Query<&MeshMaterial3d<StandardMaterial>>,
    standard_materials: Res<Assets<StandardMaterial>>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    for (avatar, mut state, view) in &mut avatars {
        let Ok((players, scene)) = views.get(view.parent()) else {
            continue;
        };
        let Some(rig) = children
            .iter_descendants(avatar)
            .find(|e| names.get(*e).is_ok_and(|n| n.as_str() == RIG_ROOT))
        else {
            continue;
        };
        if !players.paused {
            let Some((graph, _)) = &clips.graph else {
                continue;
            };
            commands.entity(rig).insert((
                AnimationPlayer::default(),
                AnimationGraphHandle(graph.clone()),
                AnimationTransitions::new(),
            ));
            tag_targets(&mut commands, rig, rig, &mut vec![], &children, &names);
            state.rig = Some(rig);
        }
        // Characters take the same hemisphere fill as every other object.
        for e in children.iter_descendants(avatar) {
            if let Some(m) = standard
                .get(e)
                .ok()
                .and_then(|m| standard_materials.get(&m.0))
            {
                commands
                    .entity(e)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert(MeshMaterial3d(materials.add(ObjectMaterial {
                        base: m.clone(),
                        extension: HemisphereFill::new(&scene.lighting),
                    })));
            }
        }
        commands.entity(avatar).insert(Bound);
    }
}

fn tag_targets(
    commands: &mut Commands,
    root: Entity,
    entity: Entity,
    path: &mut Vec<Name>,
    children: &Query<&Children>,
    names: &Query<&Name>,
) {
    let Ok(name) = names.get(entity) else {
        return;
    };
    path.push(name.clone());
    commands
        .entity(entity)
        .insert((AnimationTargetId::from_names(path.iter()), AnimatedBy(root)));
    for child in children.get(entity).into_iter().flatten() {
        tag_targets(commands, root, *child, path, children, names);
    }
    path.pop();
}

fn animate(
    views: Query<&ViewPlayers>,
    clips: Res<Clips>,
    mut avatars: Query<(&mut Avatar, &ChildOf), With<Bound>>,
    mut rigs: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some((_, nodes)) = &clips.graph else {
        return;
    };
    for (mut avatar, view) in &mut avatars {
        let Ok(players) = views.get(view.parent()) else {
            continue;
        };
        if players.paused {
            continue;
        }
        let Some(p) = players.players.iter().find(|p| p.id == avatar.id) else {
            continue;
        };
        let Some(Ok((mut player, mut transitions))) = avatar.rig.map(|r| rigs.get_mut(r)) else {
            continue;
        };
        let motion = next_motion(
            avatar.motion,
            p.vel,
            p.airborne,
            players.player_speed,
            players.run_speed,
        );
        let node = nodes[motion as usize];
        if motion != avatar.motion || !player.is_playing_animation(node) {
            avatar.motion = motion;
            transitions.play(&mut player, node, FADE).repeat();
        }
        if let Some(active) = player.animation_mut(node) {
            active.set_speed(anim_scale(
                motion,
                p.vel,
                players.player_speed,
                players.run_speed,
            ));
        }
    }
}

#[allow(clippy::type_complexity)]
fn place_tags(
    mut commands: Commands,
    views: Query<(&View, &ViewPlayers)>,
    avatars: Query<(&Avatar, &GlobalTransform, &ChildOf)>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut tags: Query<
        (
            &mut Node,
            &mut Visibility,
            &ComputedNode,
            Has<UiTargetCamera>,
        ),
        With<NameTag>,
    >,
) {
    for (avatar, transform, view) in &avatars {
        let Ok((view, players)) = views.get(view.parent()) else {
            continue;
        };
        let Some((camera_entity, (camera, camera_transform))) =
            view.camera.and_then(|c| Some((c, cameras.get(c).ok()?)))
        else {
            continue;
        };
        let Ok((mut node, mut visibility, computed, target)) = tags.get_mut(avatar.tag) else {
            continue;
        };
        // An avatar spawned before its view's camera.
        if !target {
            commands
                .entity(avatar.tag)
                .insert(UiTargetCamera(camera_entity));
        }
        let named = view.kind == ViewKind::World
            && players
                .players
                .iter()
                .any(|p| p.id == avatar.id && !p.name.is_empty());
        let at = transform.translation() + Vec3::Y * TAG_HEIGHT_M;
        match camera.world_to_viewport(camera_transform, at) {
            Ok(xy) if named && !players.paused => {
                let half = computed.size() * computed.inverse_scale_factor() / 2.0;
                node.left = Val::Px(xy.x - half.x);
                node.top = Val::Px(xy.y - half.y);
                *visibility = Visibility::Inherited;
            }
            _ => *visibility = Visibility::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(previous: Motion, speed: f32) -> Motion {
        next_motion(previous, Vec3::new(speed, 0.0, 0.0), false, 3.0, 6.0)
    }

    #[test]
    fn tiers_follow_speed_with_hysteresis() {
        assert_eq!(step(Motion::Idle, 0.07), Motion::Idle);
        assert_eq!(step(Motion::Idle, 0.08), Motion::Walking);
        assert_eq!(step(Motion::Walking, 0.06), Motion::Walking);
        assert_eq!(step(Motion::Walking, 0.05), Motion::Idle);
        assert_eq!(step(Motion::Walking, 4.6), Motion::Walking);
        assert_eq!(step(Motion::Walking, 4.8), Motion::Running);
        assert_eq!(step(Motion::Running, 4.4), Motion::Running);
        assert_eq!(step(Motion::Running, 4.2), Motion::Walking);
        assert_eq!(step(Motion::Running, 0.0), Motion::Idle);
    }

    #[test]
    fn airborne_overrides_speed() {
        let m = next_motion(Motion::Running, Vec3::new(6.0, 0.0, 0.0), true, 3.0, 6.0);
        assert_eq!(m, Motion::Jumping);
    }
}
