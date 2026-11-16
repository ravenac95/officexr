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
/// Faster than authored so a bump reads as a quick recoil, not a flinch.
const HIT_SPEED: f32 = 1.6;
const BUMP_KICK_M: f32 = 0.16;
/// Ground speed (m/s) `Walking_C` and `Running_A` are authored at: how fast a
/// planted foot moves back under the body at 1x, measured from
/// `Rig_Medium_MovementBasic.glb` (see `clip_ground_speeds_match_the_rig`).
/// Locomotion plays at the player's ground speed over these, so the feet
/// don't slide whatever the frame rate.
const WALK_CLIP_GROUND_SPEED: f32 = 0.5;
const RUN_CLIP_GROUND_SPEED: f32 = 2.43;
/// Where the ping bell hangs, above the name tag.
const BELL_HEIGHT_M: f32 = 2.45;

/// One player as the renderer needs it, as `OfficeState.players` holds it.
#[derive(Clone, Debug, Default)]
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
    /// Sitting overrides the motion `vel` and `airborne` imply, and places the player at the seat.
    pub seat: Option<Seat>,
    pub bump: Bump,
    /// The character's clip-speed tunables (`resolveCharacterTunables`'s
    /// `idleAnimSpeed` / `walkAnimSpeed` / `runAnimSpeed`).
    pub anim_speeds: AnimSpeeds,
}

/// Multipliers on the locomotion clips' playback rate. At 1 a walk or run
/// plays at the rate that keeps its planted foot still on the ground.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct AnimSpeeds {
    pub idle: f32,
    pub walk: f32,
    pub run: f32,
}

impl Default for AnimSpeeds {
    fn default() -> Self {
        Self {
            idle: 1.0,
            walk: 1.0,
            run: 1.0,
        }
    }
}

/// Where a seated player sits: the body root and yaw the seat puts them at.
/// The web frame passes the player's own `pos` and `yaw`, which the seat already set.
#[derive(Clone, Copy, Debug)]
pub struct Seat {
    pub pos: Vec3,
    pub yaw: f32,
}

/// Collision bumps a player has had; each increase of `count` is a new one,
/// pushed along `normal` (XZ, out of whoever they hit).
#[derive(Clone, Copy, Debug, Default)]
pub struct Bump {
    pub count: u32,
    pub normal: Vec2,
}

impl PlayerRender {
    fn placed(&self) -> (Vec3, f32) {
        self.seat.map_or((self.pos, self.yaw), |s| (s.pos, s.yaw))
    }
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
    /// How long a bump's push takes to ease out.
    pub bump_easing_ms: f32,
    /// Bind pose, no clips and no name tags, as the mugshot renders.
    pub paused: bool,
}

impl Players {
    /// Where `player`'s character model stands: the ball collider's bottom.
    pub fn feet(&self, player: &PlayerRender) -> Vec3 {
        player.placed().0 + Vec3::Y * (self.body_y - self.char_radius)
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
            bump_easing_ms: 180.0,
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

/// Clip time scale: ground speed (m/s) over the speed the tier's clip is
/// authored at, times the character's tunable, so a planted foot stays put.
/// Frame-rate independent: both speeds are per second. Idle plays at its
/// tunable, jumping at authored speed.
fn anim_scale(motion: Motion, vel: Vec3, speeds: AnimSpeeds) -> f32 {
    let (clip_speed, tunable) = match motion {
        Motion::Running => (RUN_CLIP_GROUND_SPEED, speeds.run),
        Motion::Walking => (WALK_CLIP_GROUND_SPEED, speeds.walk),
        Motion::Idle => return speeds.idle,
        Motion::Jumping => return 1.0,
    };
    Vec2::new(vel.x, vel.z).length() / clip_speed * tunable
}

#[derive(Component)]
pub(crate) struct Avatar {
    id: String,
    character: String,
    motion: Motion,
    playing: AnimationState,
    /// The player's `bump.count` last seen, and the running bump's start (seconds) and push.
    bumps: u32,
    bump: Option<(f32, Vec2)>,
    /// The `Rig_Medium` node, once bound.
    rig: Option<Entity>,
}

/// The label a hovered player shows to be pinged; `picking::ping_bell` drives it.
#[derive(Component, Default)]
#[cfg_attr(
    not(feature = "picking"),
    expect(dead_code, reason = "only `picking::ping_bell` reads the bell's state")
)]
pub(crate) struct PingBell {
    /// Seconds the bell stays after the pointer leaves, so it can cross to it.
    pub linger: f32,
    /// A press landed on the bell, awaiting its release.
    pub armed: bool,
}

/// Set once the character's skeleton is bound to the clips (or, paused,
/// once its materials are swapped).
#[derive(Component)]
pub struct Bound;

/// An avatar whose player changed character: it stays drawn, as
/// `SwapWhenLoaded` kept the old model, until the new one is bound.
#[derive(Component)]
pub(crate) struct Replaced;

type AvatarItem<'a> = (
    Entity,
    &'a Avatar,
    &'a mut Transform,
    &'a ChildOf,
    Has<Replaced>,
);

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
        (
            build_graph,
            sync_avatars,
            bind_avatars,
            retire_replaced,
            animate,
            show_tags,
        )
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
    let rig = gltfs
        .get(&clips.rigs[0])
        .and_then(|g| g.named_nodes.get(RIG_ROOT))
        .and_then(|n| nodes.get(n))
        .unwrap_or_else(|| panic!("no {RIG_ROOT} node in {}", RIGS[0]));
    let sit = animation_clips.add(sit_pose_clip(rig, &nodes));
    let clips_by_state =
        state_clips(find, sit).unwrap_or_else(|| panic!("no Idle_A clip in the Rig_Medium GLBs"));
    let (graph, indices) = AnimationGraph::from_clips(clips_by_state);
    clips.graph = Some((graphs.add(graph), indices.try_into().unwrap()));
}

/// Each state's clip, in [`AnimationState::ALL`] order: the sit pose for
/// sitting, and idle for a state whose clip the rigs lack, as Adventurer.tsx
/// falls back. `None` without an idle clip.
fn state_clips<T: Clone>(find: impl Fn(&str) -> Option<T>, sit: T) -> Option<[T; 8]> {
    let idle = find(AnimationState::Idle.clip()?)?;
    Some(AnimationState::ALL.map(|s| match s.clip() {
        Some(name) => find(name).unwrap_or_else(|| idle.clone()),
        None => sit.clone(),
    }))
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
    mut avatars: Query<AvatarItem>,
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
    avatars: &mut Query<AvatarItem>,
) {
    for (entity, avatar, _, _, replaced) in
        avatars.iter().filter(|(_, _, _, v, _)| v.parent() == view)
    {
        let player = players.players.iter().find(|p| p.id == avatar.id);
        match player {
            Some(p) if p.character == avatar.character => {
                if replaced {
                    commands.entity(entity).remove::<Replaced>();
                }
            }
            Some(_) => {
                commands.entity(entity).insert(Replaced);
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for p in &players.players {
        // ponytail: the yaw snaps; Players.tsx eases it at `turnSpeed`.
        let transform = Transform::from_translation(players.feet(p))
            .with_rotation(Quat::from_rotation_y(p.placed().1 + AVATAR_YAW_OFFSET));
        let existing = avatars.iter_mut().find(|(_, a, _, v, _)| {
            v.parent() == view && a.id == p.id && a.character == p.character
        });
        if let Some((_, _, mut t, _, _)) = existing {
            *t = transform;
            continue;
        }
        let replacing = avatars
            .iter()
            .any(|(_, a, _, v, _)| v.parent() == view && a.id == p.id);
        let path = base.asset_path(&character_path(&p.character));
        // ponytail: no bind-pose foot anchor; every KayKit character's lowest vertex is within 2 mm of y=0.
        let avatar = commands
            .spawn((
                Avatar {
                    id: p.id.clone(),
                    character: p.character.clone(),
                    motion: Motion::Idle,
                    playing: AnimationState::Idle,
                    bumps: p.bump.count,
                    bump: None,
                    rig: None,
                },
                PickTarget::Player { id: p.id.clone() },
                transform,
                if replacing {
                    Visibility::Hidden
                } else {
                    Visibility::default()
                },
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
        commands.spawn((
            PingBell::default(),
            world_label::icon_badge(
                assets.load("embedded://officexr_render/bell.png"),
                28.0,
                16.0,
                Color::srgb(0.07, 0.07, 0.07),
                avatar,
                Vec3::Y * BELL_HEIGHT_M,
            ),
        ));
    }
}

type NewlyBound = (Added<Bound>, Without<Replaced>);

/// Shows a newly bound avatar and drops the ones it replaced.
fn retire_replaced(
    mut commands: Commands,
    mut bound: Query<(&Avatar, &ChildOf, &mut Visibility), NewlyBound>,
    replaced: Query<(Entity, &Avatar, &ChildOf), With<Replaced>>,
) {
    for (avatar, view, mut visibility) in &mut bound {
        *visibility = Visibility::Inherited;
        for (old, old_avatar, old_view) in &replaced {
            if old_view.parent() == view.parent() && old_avatar.id == avatar.id {
                commands.entity(old).despawn();
            }
        }
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

/// The locomotion tier to remember, and the state and time scale to play:
/// the player's `animation` when it has one, else its motion's.
fn choose_clip(
    previous: Motion,
    p: &PlayerRender,
    players: &Players,
) -> (Motion, AnimationState, f32) {
    if let Some(PlayerAnimation { state, speed }) = p.animation {
        return (previous, state, speed);
    }
    // Locomotion's hysteresis restarts from idle once the player stands.
    if p.seat.is_some() {
        return (Motion::Idle, AnimationState::Sitting, 1.0);
    }
    let (walk, run) = (players.player_speed, players.run_speed);
    let motion = next_motion(previous, p.vel, p.airborne, walk, run);
    (
        motion,
        motion.into(),
        anim_scale(motion, p.vel, p.anim_speeds),
    )
}

/// Fades to `state`'s clip when it isn't what's `playing`, looping it unless
/// it is a one-shot, which then holds its last frame.
fn play(
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
    nodes: &[AnimationNodeIndex; 8],
    playing: &mut AnimationState,
    state: AnimationState,
    speed: f32,
) {
    let node = nodes[state as usize];
    if state != *playing || !player.is_playing_animation(node) {
        *playing = state;
        let active = transitions.play(player, node, FADE);
        if state.loops() {
            active.repeat();
        }
    }
    if let Some(active) = player.animation_mut(node) {
        active.set_speed(speed);
    }
}

#[allow(clippy::type_complexity)]
fn animate(
    time: Res<Time>,
    views: Query<&ViewPlayers>,
    clips: Res<Clips>,
    mut avatars: Query<(&mut Avatar, &mut Transform, &ChildOf), With<Bound>>,
    mut rigs: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some((_, nodes)) = &clips.graph else {
        return;
    };
    let now = time.elapsed_secs();
    for (mut avatar, mut transform, view) in &mut avatars {
        let Ok(players) = views.get(view.parent()) else {
            continue;
        };
        if players.paused {
            continue;
        }
        let Some(p) = players.players.iter().find(|p| p.id == avatar.id) else {
            continue;
        };
        let bumped = p.bump.count != avatar.bumps;
        if bumped {
            avatar.bumps = p.bump.count;
            avatar.bump = Some((now, p.bump.normal));
        }
        // Players.tsx's visual-only kick: a decaying wave along the bump's normal.
        let mut kick = Vec3::ZERO;
        if let Some((start, normal)) = avatar.bump {
            let t = (now - start) * 1000.0 / players.bump_easing_ms;
            if t >= 1.0 {
                avatar.bump = None;
            } else {
                let env = (-3.2 * t).exp() * (2.0 * PI * 1.1 * t).cos();
                kick = Vec3::new(normal.x, 0.0, normal.y) * env * BUMP_KICK_M;
            }
        }
        transform.translation = players.feet(p) + kick;
        let Some(Ok((mut player, mut transitions))) = avatar.rig.map(|r| rigs.get_mut(r)) else {
            continue;
        };
        let (motion, state, speed) = choose_clip(avatar.motion, p, &players.0);
        avatar.motion = motion;
        let hit = nodes[AnimationState::Hit as usize];
        if bumped && p.animation.is_none() && state != AnimationState::Sitting {
            play(
                &mut player,
                &mut transitions,
                nodes,
                &mut avatar.playing,
                AnimationState::Hit,
                HIT_SPEED,
            );
            // A bump during a reaction starts it over, as Adventurer.tsx does.
            if let Some(active) = player.animation_mut(hit) {
                active.replay();
            }
        }
        // A bump's reaction plays over the loop, which resumes when it ends.
        let reacting = avatar.playing == AnimationState::Hit && p.animation.is_none();
        if reacting && player.animation(hit).is_some_and(|a| !a.is_finished()) {
            continue;
        }
        play(
            &mut player,
            &mut transitions,
            nodes,
            &mut avatar.playing,
            state,
            speed,
        );
    }
}

/// Shows a named player's tag in a world view whose players aren't paused.
fn show_tags(
    views: Query<(&View, &ViewPlayers)>,
    avatars: Query<(&Avatar, &Labels, &ChildOf)>,
    mut tags: Query<&mut WorldLabel, Without<PingBell>>,
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
    use bevy::animation::RepeatAnimation;

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

    fn player(vel: Vec3, animation: Option<PlayerAnimation>) -> PlayerRender {
        PlayerRender {
            id: "p".into(),
            character: "Barbarian".into(),
            pos: Vec3::ZERO,
            yaw: 0.0,
            vel,
            name: String::new(),
            airborne: false,
            animation,
            ..default()
        }
    }

    #[test]
    fn an_animation_plays_its_state_at_its_speed_and_leaves_the_tier_alone() {
        let shoot = PlayerAnimation {
            state: AnimationState::Shooting,
            speed: 0.5,
        };
        let p = player(Vec3::new(6.0, 0.0, 0.0), Some(shoot));
        let chosen = choose_clip(Motion::Walking, &p, &Players::default());
        assert_eq!(chosen, (Motion::Walking, AnimationState::Shooting, 0.5));
    }

    #[test]
    fn a_seat_overrides_motion_but_not_an_animation_preview() {
        let seat = Seat {
            pos: Vec3::new(1.0, 2.0, 3.0),
            yaw: 0.5,
        };
        let mut p = player(Vec3::new(6.0, 0.0, 0.0), None);
        p.airborne = true;
        p.seat = Some(seat);
        let players = Players::default();
        let chosen = choose_clip(Motion::Running, &p, &players);
        assert_eq!(chosen, (Motion::Idle, AnimationState::Sitting, 1.0));
        assert_eq!(players.feet(&p), Vec3::new(1.0, 2.5, 3.0));
        assert_eq!(p.placed().1, 0.5);

        let shoot = PlayerAnimation {
            state: AnimationState::Shooting,
            speed: 1.0,
        };
        p.animation = Some(shoot);
        assert_eq!(
            choose_clip(Motion::Idle, &p, &players).1,
            AnimationState::Shooting
        );
    }

    #[test]
    fn without_an_animation_the_velocity_picks_the_clip_as_before() {
        let p = player(Vec3::new(3.0, 0.0, 0.0), None);
        let chosen = choose_clip(Motion::Idle, &p, &Players::default());
        assert_eq!(chosen.0, Motion::Walking);
        assert_eq!(chosen.1, AnimationState::Walking);
        assert!((chosen.2 - 3.0 / WALK_CLIP_GROUND_SPEED).abs() < 1e-5);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn locomotion_plays_at_ground_speed_over_the_clips_authored_speed() {
        let ones = AnimSpeeds::default();
        let walk = |v: Vec3| anim_scale(Motion::Walking, v, ones);
        // At the clip's own ground speed it plays at 1x, in any direction.
        assert!(close(
            walk(Vec3::new(0.0, 0.0, WALK_CLIP_GROUND_SPEED)),
            1.0
        ));
        assert!(close(
            walk(Vec3::new(0.3, 0.0, 0.4)),
            0.5 / WALK_CLIP_GROUND_SPEED
        ));
        // The default 3 m/s walk and 6 m/s run play well over 1x, rising
        // linearly with speed rather than clamping at the tier's reference.
        assert!(close(walk(Vec3::X * 3.0), 6.0));
        assert!(close(walk(Vec3::X * 4.5), 9.0));
        let run = anim_scale(Motion::Running, Vec3::X * 6.0, ones);
        assert!(close(run, 6.0 / RUN_CLIP_GROUND_SPEED));
        // Vertical speed (a slope, a fall) doesn't speed the legs up.
        assert!(close(walk(Vec3::new(3.0, -5.0, 0.0)), 6.0));
        assert!(close(anim_scale(Motion::Idle, Vec3::X, ones), 1.0));
        assert!(close(anim_scale(Motion::Jumping, Vec3::X * 6.0, ones), 1.0));
    }

    #[test]
    fn the_characters_tunables_scale_their_clip() {
        let speeds = AnimSpeeds {
            idle: 0.5,
            walk: 0.8,
            run: 1.25,
        };
        let walk = anim_scale(Motion::Walking, Vec3::X * 3.0, speeds);
        assert!(close(walk, 3.0 / WALK_CLIP_GROUND_SPEED * 0.8));
        let run = anim_scale(Motion::Running, Vec3::X * 6.0, speeds);
        assert!(close(run, 6.0 / RUN_CLIP_GROUND_SPEED * 1.25));
        assert!(close(anim_scale(Motion::Idle, Vec3::ZERO, speeds), 0.5));
        assert!(close(anim_scale(Motion::Jumping, Vec3::ZERO, speeds), 1.0));

        let mut p = player(Vec3::X * 3.0, None);
        p.anim_speeds = speeds;
        let chosen = choose_clip(Motion::Walking, &p, &Players::default());
        assert!(close(chosen.2, walk));
    }

    /// Measures how fast a planted foot slides back under the body in a
    /// `Rig_Medium_MovementBasic.glb` clip at 1x: the clip's ground speed.
    fn clip_ground_speed(clip: &str) -> f32 {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/studio/public/models/animations/Rig_Medium_MovementBasic.glb"
        );
        let glb = std::fs::read(path).expect("the rig GLB is checked in");
        let le = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
        let json_len = le(12);
        let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let bin = &glb[20 + json_len + 8..];
        let floats = |accessor: &serde_json::Value| -> Vec<f32> {
            let a = &doc["accessors"][accessor.as_u64().unwrap() as usize];
            assert_eq!(a["componentType"], 5126, "float keys");
            let view = &doc["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
            let width = match a["type"].as_str().unwrap() {
                "SCALAR" => 1,
                "VEC3" => 3,
                _ => 4,
            };
            let start = (view["byteOffset"].as_u64().unwrap_or(0)
                + a["byteOffset"].as_u64().unwrap_or(0)) as usize;
            let n = a["count"].as_u64().unwrap() as usize * width;
            (0..n)
                .map(|i| {
                    f32::from_le_bytes(bin[start + i * 4..start + i * 4 + 4].try_into().unwrap())
                })
                .collect()
        };
        let nodes = doc["nodes"].as_array().unwrap();
        let mut parent = vec![None; nodes.len()];
        for (i, n) in nodes.iter().enumerate() {
            for c in n["children"].as_array().into_iter().flatten() {
                parent[c.as_u64().unwrap() as usize] = Some(i);
            }
        }
        let anim = doc["animations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == clip)
            .unwrap();
        let mut curves = std::collections::HashMap::new();
        let mut duration = 0.0f32;
        for ch in anim["channels"].as_array().unwrap() {
            let s = &anim["samplers"][ch["sampler"].as_u64().unwrap() as usize];
            let times = floats(&s["input"]);
            duration = duration.max(*times.last().unwrap());
            let key = (
                ch["target"]["node"].as_u64().unwrap() as usize,
                ch["target"]["path"].as_str().unwrap().to_string(),
            );
            curves.insert(key, (times, floats(&s["output"])));
        }
        let sample = |node: usize, path: &str, t: f32, width: usize| -> Option<Vec<f32>> {
            let (times, values) = curves.get(&(node, path.to_string()))?;
            let k = times.partition_point(|&x| x <= t).clamp(1, times.len() - 1) - 1;
            let f = ((t - times[k]) / (times[k + 1] - times[k])).clamp(0.0, 1.0);
            let at = |i: usize| values[i * width..(i + 1) * width].to_vec();
            Some(if width == 4 {
                let q = |v: Vec<f32>| Quat::from_xyzw(v[0], v[1], v[2], v[3]);
                q(at(k)).slerp(q(at(k + 1)), f).to_array().to_vec()
            } else {
                let (a, b) = (Vec3::from_slice(&at(k)), Vec3::from_slice(&at(k + 1)));
                a.lerp(b, f).to_array().to_vec()
            })
        };
        let rest = |node: usize, key: &str, default: &[f32]| -> Vec<f32> {
            nodes[node][key].as_array().map_or(default.to_vec(), |v| {
                v.iter().map(|x| x.as_f64().unwrap() as f32).collect()
            })
        };
        let world = |mut node: usize, t: f32| -> Vec3 {
            let mut m = Mat4::IDENTITY;
            loop {
                let tr = sample(node, "translation", t, 3)
                    .unwrap_or_else(|| rest(node, "translation", &[0.0; 3]));
                let r = sample(node, "rotation", t, 4)
                    .unwrap_or_else(|| rest(node, "rotation", &[0.0, 0.0, 0.0, 1.0]));
                let s =
                    sample(node, "scale", t, 3).unwrap_or_else(|| rest(node, "scale", &[1.0; 3]));
                let local = Mat4::from_scale_rotation_translation(
                    Vec3::from_slice(&s),
                    Quat::from_slice(&r),
                    Vec3::from_slice(&tr),
                );
                m = local * m;
                match parent[node] {
                    Some(p) => node = p,
                    None => return m.w_axis.truncate(),
                }
            }
        };
        let foot = nodes.iter().position(|n| n["name"] == "foot.l").unwrap();
        const N: usize = 160;
        let dt = duration / N as f32;
        let feet: Vec<Vec3> = (0..=N).map(|i| world(foot, i as f32 * dt)).collect();
        // While planted (its lowest 40 % of heights) the foot slides back at
        // the clip's ground speed.
        let mut heights: Vec<f32> = feet.iter().map(|p| p.y).collect();
        heights.sort_by(f32::total_cmp);
        let planted = heights[N * 2 / 5];
        let mut speeds: Vec<f32> = (1..N)
            .filter(|&i| feet[i].y <= planted)
            .map(|i| (feet[i - 1].z - feet[i + 1].z).abs() / (2.0 * dt))
            .collect();
        speeds.sort_by(f32::total_cmp);
        speeds[speeds.len() / 2]
    }

    #[test]
    fn clip_ground_speeds_match_the_rig() {
        for (clip, authored) in [
            (AnimationState::Walking, WALK_CLIP_GROUND_SPEED),
            (AnimationState::Running, RUN_CLIP_GROUND_SPEED),
        ] {
            let measured = clip_ground_speed(clip.clip().unwrap());
            assert!(
                (measured - authored).abs() / authored < 0.05,
                "{clip:?}: measured {measured} m/s, constant {authored} m/s"
            );
        }
    }

    #[test]
    fn missing_clips_play_idle_and_sitting_plays_the_sit_pose() {
        let rigs = |name: &str| {
            ["Idle_A", "Walking_C", "Hit_A"]
                .contains(&name)
                .then(|| name.to_string())
        };
        let clips = state_clips(rigs, "sit".to_string()).unwrap();
        let of = |s: AnimationState| clips[s as usize].as_str();
        assert_eq!(of(AnimationState::Walking), "Walking_C");
        assert_eq!(of(AnimationState::Hit), "Hit_A");
        assert_eq!(of(AnimationState::Shooting), "Idle_A");
        assert_eq!(of(AnimationState::Sitting), "sit");
        assert!(state_clips(|_| None::<String>, "sit".into()).is_none());
    }

    #[test]
    fn one_shots_play_once_and_hold_and_loops_repeat() {
        let mut graph = AnimationGraph::new();
        let nodes: [AnimationNodeIndex; 8] =
            std::array::from_fn(|_| graph.add_clip(Handle::default(), 1.0, graph.root));
        let mut player = AnimationPlayer::default();
        let mut transitions = AnimationTransitions::new();
        let mut playing = AnimationState::Idle;
        let mut play_state = |player: &mut AnimationPlayer, state| {
            play(player, &mut transitions, &nodes, &mut playing, state, 2.0);
            *player.animation(nodes[state as usize]).unwrap()
        };

        let idle = play_state(&mut player, AnimationState::Idle);
        assert_eq!(idle.repeat_mode(), RepeatAnimation::Forever);
        let shot = play_state(&mut player, AnimationState::Shooting);
        assert_eq!(shot.repeat_mode(), RepeatAnimation::Never);
        assert_eq!(shot.speed(), 2.0);

        // Finished, it stays in the player at its end rather than restarting.
        player
            .animation_mut(nodes[AnimationState::Shooting as usize])
            .unwrap()
            .seek_to(1.0);
        let held = play_state(&mut player, AnimationState::Shooting);
        assert_eq!(held.seek_time(), 1.0);
    }

    #[test]
    fn the_sit_pose_holds_every_rig_bone() {
        let mut nodes = Assets::<GltfNode>::default();
        let node = |name: &str, children| GltfNode {
            index: 0,
            name: name.into(),
            children,
            mesh: None,
            skin: None,
            transform: Transform::from_xyz(0.0, 0.5, 0.0),
            is_animation_root: false,
            extras: None,
        };
        let leg = nodes.add(node("upperleg.l", vec![]));
        let hips = nodes.add(node("hips", vec![leg]));
        let rig = node(RIG_ROOT, vec![hips]);
        let clip = sit_pose_clip(&rig, &nodes);

        let target = |path: &[&str]| {
            AnimationTargetId::from_names(
                path.iter()
                    .map(|n| Name::new(n.to_string()))
                    .collect::<Vec<_>>()
                    .iter(),
            )
        };
        for path in [
            &[RIG_ROOT][..],
            &[RIG_ROOT, "hips"],
            &[RIG_ROOT, "hips", "upperleg.l"],
        ] {
            assert_eq!(
                clip.curves_for_target(target(path)).map(Vec::len),
                Some(3),
                "{path:?}"
            );
        }
        assert_eq!(clip.duration(), 1.0);
    }

    #[test]
    fn a_changed_character_keeps_the_old_model_until_the_new_one_binds() {
        use crate::views::{ViewHub, ViewKind, ViewMsg, ViewTarget};
        let mut app = crate::tests::bare_app();
        app.init_asset::<WorldAsset>().add_systems(
            Update,
            (sync_avatars, retire_replaced)
                .chain()
                .after(crate::views::mirror_resources),
        );
        let hub = app.world().resource::<ViewHub>().clone();
        let id = hub.create(ViewKind::Preview, |_| ViewTarget::Window("#c".into()));
        let as_character = |character: &str| Players {
            players: vec![PlayerRender {
                character: character.into(),
                ..player(Vec3::ZERO, None)
            }],
            ..default()
        };
        let avatars = |app: &mut App| {
            let mut q = app
                .world_mut()
                .query::<(Entity, &Avatar, &Visibility, Has<Replaced>)>();
            let mut all: Vec<_> = q
                .iter(app.world())
                .map(|(e, a, v, r)| (e, a.character.clone(), *v, r))
                .collect();
            all.sort_by(|a, b| a.1.cmp(&b.1));
            all
        };

        hub.send(id, ViewMsg::Players(as_character("Barbarian")));
        app.update();
        let (old, ..) = avatars(&mut app)[0];
        app.world_mut().entity_mut(old).insert(Bound);
        hub.send(id, ViewMsg::Players(as_character("Knight")));
        app.update();
        let both = avatars(&mut app);
        assert_eq!(both.len(), 2);
        assert!(both[0].3, "the Barbarian is replaced but kept");
        assert_eq!(both[1].2, Visibility::Hidden, "the Knight waits hidden");

        let new = both[1].0;
        app.world_mut().entity_mut(new).insert(Bound);
        app.update();
        let left = avatars(&mut app);
        assert_eq!(left.len(), 1);
        assert_eq!((left[0].0, left[0].2), (new, Visibility::Inherited));
    }
}
