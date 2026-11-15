//! Port of `Players.tsx` + `Adventurer.tsx` + `PlayerNameTag.tsx`: one
//! KayKit character per player, animated with the shared `Rig_Medium_*`
//! clips, with a screen-space name tag over its head.

use std::f32::consts::PI;
use std::time::Duration;

use bevy::animation::animation_curves::{AnimatableCurve, AnimatableKeyframeCurve};
use bevy::animation::{AnimatedBy, AnimationTargetId, animated_field};
use bevy::gltf::Gltf;
use bevy::prelude::*;

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
const HIT_FADE: Duration = Duration::from_millis(50);
const BUMP_KICK_M: f32 = 0.16;
/// Where the ping bell hangs, above the name tag.
const BELL_HEIGHT_M: f32 = 2.45;

/// Rig_Medium bone paths below `root` and their seated local rotations
/// (XYZ Euler degrees), as `sitPoseClip.ts` poses them; other bones keep what they had.
const SIT_POSE: [(&str, [f32; 3]); 11] = [
    ("hips/upperleg.l", [90.0, 0.0, 0.0]),
    ("hips/upperleg.r", [90.0, 0.0, 0.0]),
    ("hips/upperleg.l/lowerleg.l", [90.0, 0.0, 0.0]),
    ("hips/upperleg.r/lowerleg.r", [90.0, 0.0, 0.0]),
    ("hips/upperleg.l/lowerleg.l/foot.l", [-44.0, 0.0, 0.0]),
    ("hips/upperleg.r/lowerleg.r/foot.r", [-44.0, 0.0, 0.0]),
    ("hips/spine", [8.0, 0.0, 0.0]),
    // Idle_A frame 0: the bind pose is a T-pose.
    ("hips/spine/chest/upperarm.l", [-52.6, 48.4, -109.2]),
    ("hips/spine/chest/upperarm.r", [-53.1, -52.3, 98.5]),
    ("hips/spine/chest/upperarm.l/lowerarm.l", [0.0, 0.0, -42.0]),
    ("hips/spine/chest/upperarm.r/lowerarm.r", [0.0, 0.0, 44.0]),
];

/// Where a seated player sits: the body root and yaw the seat puts them at.
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

/// One player as the renderer needs it, as `OfficeState.players` holds it.
#[derive(Clone, Debug, Default)]
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
    /// Sitting overrides every other motion, and places the player at the seat.
    pub seat: Option<Seat>,
    pub bump: Bump,
}

impl PlayerRender {
    fn placed(&self) -> (Vec3, f32) {
        self.seat.map_or((self.pos, self.yaw), |s| (s.pos, s.yaw))
    }
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
    Sitting,
}

/// Graph node of the one-shot hit reaction, after one node per [`Motion`].
const HIT_NODE: usize = Motion::Sitting as usize + 1;

/// `Players.tsx`'s locomotion tier, with its enter/exit hysteresis.
pub fn next_motion(
    previous: Motion,
    vel: Vec3,
    airborne: bool,
    seated: bool,
    player_speed: f32,
    run_speed: f32,
) -> Motion {
    if seated {
        return Motion::Sitting;
    }
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
        Motion::Idle | Motion::Jumping | Motion::Sitting if speed_sq >= run_enter => {
            Motion::Running
        }
        Motion::Idle | Motion::Jumping | Motion::Sitting if speed_sq >= walk_enter => {
            Motion::Walking
        }
        Motion::Idle | Motion::Jumping | Motion::Sitting => Motion::Idle,
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
        Motion::Idle | Motion::Jumping | Motion::Sitting => return 1.0,
    };
    let ratio = if denom > 0.0 {
        Vec2::new(vel.x, vel.z).length() / denom
    } else {
        0.0
    };
    (ratio.clamp(0.0, 1.0) * 10.0).round() / 10.0
}

#[derive(Component)]
pub(crate) struct Avatar {
    id: String,
    character: String,
    motion: Motion,
    /// The player's `bump.count` last seen, and the running bump's start (seconds) and push.
    bumps: u32,
    bump: Option<(f32, Vec2)>,
    /// The `Rig_Medium` node, once bound.
    rig: Option<Entity>,
}

/// The label a hovered player shows to be pinged; `picking::ping_bell` drives it.
#[derive(Component, Default)]
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

#[derive(Resource)]
struct Clips {
    rigs: Vec<Handle<Gltf>>,
    graph: Option<(Handle<AnimationGraph>, [AnimationNodeIndex; HIT_NODE + 1])>,
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
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut animations: ResMut<Assets<AnimationClip>>,
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
    let (graph, nodes) = AnimationGraph::from_clips([
        find("Idle_A"),
        find("Walking_C"),
        find("Running_A"),
        find("Jump_Full_Long"),
        animations.add(sit_pose_clip()),
        find("Hit_A"),
    ]);
    clips.graph = Some((graphs.add(graph), nodes.try_into().unwrap()));
}

fn sit_pose_clip() -> AnimationClip {
    let mut clip = AnimationClip::default();
    for (bone, [x, y, z]) in SIT_POSE {
        let path: Vec<Name> = ["Rig_Medium", "root"]
            .into_iter()
            .chain(bone.split('/'))
            .map(Name::new)
            .collect();
        let rotation = Quat::from_euler(
            EulerRot::XYZ,
            x.to_radians(),
            y.to_radians(),
            z.to_radians(),
        );
        clip.add_curve_to_target(
            AnimationTargetId::from_names(path.iter()),
            AnimatableCurve::new(
                animated_field!(Transform::rotation),
                AnimatableKeyframeCurve::new([(0.0, rotation), (1.0, rotation)])
                    .expect("two keyframes"),
            ),
        );
    }
    clip
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
            .with_rotation(Quat::from_rotation_y(p.placed().1 + AVATAR_YAW_OFFSET));
        let existing = avatars
            .iter_mut()
            .find(|(_, a, _, v)| v.parent() == view && a.id == p.id && a.character == p.character);
        if let Some((_, _, mut t, _)) = existing {
            *t = transform;
            continue;
        }
        let path = base.asset_path(&format!("/models/characters/{}.glb", p.character));
        // ponytail: no bind-pose foot anchor; every KayKit character's lowest vertex is within 2 mm of y=0.
        let avatar = commands
            .spawn((
                Avatar {
                    id: p.id.clone(),
                    character: p.character.clone(),
                    motion: Motion::Idle,
                    bumps: p.bump.count,
                    bump: None,
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
        commands.spawn((
            PingBell::default(),
            world_label::label(
                "Ping".into(),
                14.0,
                Color::srgb(0.07, 0.07, 0.07),
                avatar,
                Vec3::Y * BELL_HEIGHT_M,
            ),
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
        if p.bump.count != avatar.bumps {
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
        let motion = next_motion(
            avatar.motion,
            p.vel,
            p.airborne,
            p.seat.is_some(),
            players.player_speed,
            players.run_speed,
        );
        let hit = nodes[HIT_NODE];
        if motion != Motion::Sitting && avatar.bump.is_some_and(|(start, _)| start == now) {
            transitions
                .play(&mut player, hit, HIT_FADE)
                .set_speed(HIT_SPEED);
        }
        // The reaction plays over the loop, which resumes when it ends.
        if player.animation(hit).is_some_and(|a| !a.is_finished()) {
            continue;
        }
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
    use super::*;

    fn step(previous: Motion, speed: f32) -> Motion {
        next_motion(previous, Vec3::new(speed, 0.0, 0.0), false, false, 3.0, 6.0)
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
        let m = next_motion(
            Motion::Running,
            Vec3::new(6.0, 0.0, 0.0),
            true,
            false,
            3.0,
            6.0,
        );
        assert_eq!(m, Motion::Jumping);
    }

    #[test]
    fn sitting_overrides_airborne_and_speed_and_restarts_the_tiers() {
        let fast = Vec3::new(6.0, 0.0, 0.0);
        let m = next_motion(Motion::Running, fast, true, true, 3.0, 6.0);
        assert_eq!(m, Motion::Sitting);
        assert_eq!(step(Motion::Sitting, 0.0), Motion::Idle);
        assert_eq!(step(Motion::Sitting, 0.08), Motion::Walking);
    }

    #[test]
    fn a_seat_places_the_avatar_at_the_seat() {
        let seat = Seat {
            pos: Vec3::new(1.0, 2.0, 3.0),
            yaw: 0.5,
        };
        let p = PlayerRender {
            seat: Some(seat),
            ..default()
        };
        assert_eq!(Players::default().feet(&p), Vec3::new(1.0, 2.5, 3.0));
        assert_eq!(p.placed().1, 0.5);
    }
}
