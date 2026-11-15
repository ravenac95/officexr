//! Port of `Players.tsx` + `Adventurer.tsx` + `PlayerNameTag.tsx`: one
//! KayKit character per player, animated with the shared `Rig_Medium_*`
//! clips, with a screen-space name tag over its head.

use std::f32::consts::PI;
use std::time::Duration;

use bevy::animation::animated_field;
use bevy::animation::animation_curves::{AnimatableCurve, AnimatableKeyframeCurve};
use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::gltf::{Gltf, GltfNode};
use bevy::prelude::*;
use serde::Deserialize;

use crate::AssetBase;
use crate::lighting::{HemisphereFill, ObjectMaterial};
use crate::picking::PickTarget;
use crate::views::{View, ViewKind, ViewPlayers, ViewScene};
use crate::world_label::{self, Labels, WorldLabel};

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
    /// A `/models/characters/<character>.glb` name, e.g. `Barbarian`, or a
    /// model's path or URL, as an uploaded character's.
    pub character: String,
    /// Body root: the ball collider's bottom sits `BODY_Y - char_radius` above it.
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: Vec3,
    /// Empty hides the tag, as for the local player.
    pub name: String,
    pub airborne: bool,
    /// Plays this instead of the motion `vel` and `airborne` imply, as the character editor previews.
    pub animation: Option<PlayerAnimation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct PlayerAnimation {
    pub state: AnimationState,
    /// The clip's time scale.
    pub speed: f32,
}

/// `Adventurer.tsx`'s `AnimationState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationState {
    Idle,
    Walking,
    Running,
    Jumping,
    Shooting,
    Throwing,
    Hit,
    Sitting,
}

impl AnimationState {
    const ALL: [AnimationState; 8] = [
        AnimationState::Idle,
        AnimationState::Walking,
        AnimationState::Running,
        AnimationState::Jumping,
        AnimationState::Shooting,
        AnimationState::Throwing,
        AnimationState::Hit,
        AnimationState::Sitting,
    ];

    /// The rig clip, or `None` for the sit pose, which no rig carries.
    fn clip(self) -> Option<&'static str> {
        Some(match self {
            AnimationState::Idle => "Idle_A",
            AnimationState::Walking => "Walking_C",
            AnimationState::Running => "Running_A",
            AnimationState::Jumping => "Jump_Full_Long",
            AnimationState::Shooting => "1H_Ranged_Shoot",
            AnimationState::Throwing => "2H_Ranged_Throwing",
            AnimationState::Hit => "Hit_A",
            AnimationState::Sitting => return None,
        })
    }

    /// The rest play once and hold their last frame.
    fn loops(self) -> bool {
        !matches!(
            self,
            AnimationState::Shooting | AnimationState::Throwing | AnimationState::Hit
        )
    }
}

impl From<Motion> for AnimationState {
    fn from(motion: Motion) -> Self {
        match motion {
            Motion::Idle => AnimationState::Idle,
            Motion::Walking => AnimationState::Walking,
            Motion::Running => AnimationState::Running,
            Motion::Jumping => AnimationState::Jumping,
        }
    }
}

/// `sitPoseClip.ts`'s seated pose: absolute local XYZ Euler rotations in degrees.
const SIT_POSE_DEGREES: [(&str, [f32; 3]); 11] = [
    ("upperleg.l", [90.0, 0.0, 0.0]),
    ("upperleg.r", [90.0, 0.0, 0.0]),
    ("lowerleg.l", [90.0, 0.0, 0.0]),
    ("lowerleg.r", [90.0, 0.0, 0.0]),
    ("foot.l", [-44.0, 0.0, 0.0]),
    ("foot.r", [-44.0, 0.0, 0.0]),
    ("spine", [8.0, 0.0, 0.0]),
    // Idle_A frame 0: the bind pose is a T-pose.
    ("upperarm.l", [-52.6, 48.4, -109.2]),
    ("upperarm.r", [-53.1, -52.3, 98.5]),
    ("lowerarm.l", [0.0, 0.0, -42.0]),
    ("lowerarm.r", [0.0, 0.0, 44.0]),
];

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
    playing: AnimationState,
    /// The `Rig_Medium` node, once bound.
    rig: Option<Entity>,
}

/// Set once the character's skeleton is bound to the clips (or, paused,
/// once its materials are swapped).
#[derive(Component)]
pub struct Bound;

#[derive(Resource)]
struct Clips {
    rigs: Vec<Handle<Gltf>>,
    graph: Option<(Handle<AnimationGraph>, [AnimationNodeIndex; 8])>,
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
    app.add_systems(Startup, load_rigs).add_systems(
        Update,
        (build_graph, sync_avatars, bind_avatars, animate, show_tags)
            .chain()
            .in_set(crate::views::SceneSystems)
            .after(crate::views::mirror_resources)
            .before(world_label::target_labels),
    );
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
    nodes: Res<Assets<GltfNode>>,
    mut animation_clips: ResMut<Assets<AnimationClip>>,
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
    };
    let idle = find(AnimationState::Idle.clip().unwrap())
        .unwrap_or_else(|| panic!("no Idle_A clip in the Rig_Medium GLBs"));
    let rig = gltfs
        .get(&clips.rigs[0])
        .and_then(|g| g.named_nodes.get(RIG_ROOT))
        .and_then(|n| nodes.get(n))
        .unwrap_or_else(|| panic!("no {RIG_ROOT} node in {}", RIGS[0]));
    let sit = animation_clips.add(sit_pose_clip(rig, &nodes));
    let (graph, indices) = AnimationGraph::from_clips(
        // A state whose clip the rigs lack plays idle, as Adventurer.tsx falls back.
        AnimationState::ALL.map(|s| match s.clip() {
            Some(name) => find(name).unwrap_or_else(|| idle.clone()),
            None => sit.clone(),
        }),
    );
    clips.graph = Some((graphs.add(graph), indices.try_into().unwrap()));
}

/// Holds every bone under `rig` at its rest transform, except the seated
/// pose's, as `sitPoseClip.ts` leaves the bones it doesn't list at their bind pose.
fn sit_pose_clip(rig: &GltfNode, nodes: &Assets<GltfNode>) -> AnimationClip {
    let mut clip = AnimationClip::default();
    let mut path = vec![Name::new(rig.name.clone())];
    add_sit_curves(&mut clip, rig, &mut path, nodes);
    clip
}

/// Two keys, as a curve needs, a second apart like the TS clip's.
fn held<T: Animatable + Copy>(value: T) -> AnimatableKeyframeCurve<T> {
    AnimatableKeyframeCurve::new([(0.0, value), (1.0, value)]).expect("two ordered keys")
}

fn add_sit_curves(
    clip: &mut AnimationClip,
    node: &GltfNode,
    path: &mut Vec<Name>,
    nodes: &Assets<GltfNode>,
) {
    let target = AnimationTargetId::from_names(path.iter());
    let rest = node.transform;
    let rotation = SIT_POSE_DEGREES
        .iter()
        .find(|(bone, _)| *bone == node.name)
        .map_or(rest.rotation, |(_, [x, y, z])| {
            Quat::from_euler(
                EulerRot::XYZ,
                x.to_radians(),
                y.to_radians(),
                z.to_radians(),
            )
        });
    clip.add_curve_to_target(
        target,
        AnimatableCurve::new(
            animated_field!(Transform::translation),
            held(rest.translation),
        ),
    );
    clip.add_curve_to_target(
        target,
        AnimatableCurve::new(animated_field!(Transform::rotation), held(rotation)),
    );
    clip.add_curve_to_target(
        target,
        AnimatableCurve::new(animated_field!(Transform::scale), held(rest.scale)),
    );
    for child in node.children.iter().filter_map(|c| nodes.get(c)) {
        path.push(Name::new(child.name.clone()));
        add_sit_curves(clip, child, path, nodes);
        path.pop();
    }
}

fn sync_avatars(
    mut commands: Commands,
    views: Query<(Entity, &ViewPlayers), Changed<ViewPlayers>>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    mut avatars: Query<(Entity, &Avatar, &mut Transform, &ChildOf)>,
) {
    for (view, players) in &views {
        sync_view_avatars(&mut commands, view, players, &base, &assets, &mut avatars);
    }
}

fn sync_view_avatars(
    commands: &mut Commands,
    view: Entity,
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
        let path = base.asset_path(&character_path(&p.character));
        // ponytail: no bind-pose foot anchor; every KayKit character's lowest vertex is within 2 mm of y=0.
        let avatar = commands
            .spawn((
                Avatar {
                    id: p.id.clone(),
                    character: p.character.clone(),
                    motion: Motion::Idle,
                    playing: AnimationState::Idle,
                    rig: None,
                },
                PickTarget::Player { id: p.id.clone() },
                transform,
                Visibility::default(),
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                ChildOf(view),
            ))
            .id();
        commands.spawn(world_label::label(
            p.name.clone(),
            14.0,
            Color::srgba(0.0, 0.0, 0.0, 0.55),
            avatar,
            Vec3::Y * TAG_HEIGHT_M,
        ));
    }
}

fn character_path(character: &str) -> String {
    if character.contains('/') {
        character.to_string()
    } else {
        format!("/models/characters/{character}.glb")
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
        let (state, speed) = match p.animation {
            Some(PlayerAnimation { state, speed }) => (state, speed),
            None => {
                let motion = next_motion(
                    avatar.motion,
                    p.vel,
                    p.airborne,
                    players.player_speed,
                    players.run_speed,
                );
                avatar.motion = motion;
                let speed = anim_scale(motion, p.vel, players.player_speed, players.run_speed);
                (motion.into(), speed)
            }
        };
        let node = nodes[state as usize];
        // A one-shot clip that has finished stays in the player, holding its last frame.
        if state != avatar.playing || !player.is_playing_animation(node) {
            avatar.playing = state;
            let active = transitions.play(&mut player, node, FADE);
            if state.loops() {
                active.repeat();
            }
        }
        if let Some(active) = player.animation_mut(node) {
            active.set_speed(speed);
        }
    }
}

/// Shows a named player's tag in a world view whose players aren't paused.
fn show_tags(
    views: Query<(&View, &ViewPlayers)>,
    avatars: Query<(&Avatar, &Labels, &ChildOf)>,
    mut tags: Query<&mut WorldLabel>,
) {
    for (avatar, labels, view) in &avatars {
        let Ok((view, players)) = views.get(view.parent()) else {
            continue;
        };
        let shown = view.kind == ViewKind::World
            && !players.paused
            && players
                .players
                .iter()
                .any(|p| p.id == avatar.id && !p.name.is_empty());
        for tag in labels.iter() {
            if let Ok(mut tag) = tags.get_mut(tag)
                && tag.shown != shown
            {
                tag.shown = shown;
            }
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
