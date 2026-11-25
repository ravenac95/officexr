//! Port of `pokeClip.ts` and `castClip.ts`: short right-arm clips played over
//! whatever the body plays, with the arm weighted to win.

use bevy::animation::animated_field;
use bevy::animation::animation_curves::{AnimatableCurve, AnimatableKeyframeCurve};
use bevy::animation::{AnimationTargetId, RepeatAnimation};
use bevy::gltf::GltfNode;
use bevy::prelude::*;

/// XYZ Euler degrees per bone.
type Pose = [(&'static str, [f32; 3]); 2];

const REST: Pose = [
    ("upperarm.r", [-53.1, -52.3, 98.5]),
    ("lowerarm.r", [0.0, 0.0, 44.0]),
];
const EXTENDED: Pose = [
    ("upperarm.r", [-96.8, 7.2, -180.0]),
    ("lowerarm.r", [0.0, 0.0, 6.3]),
];
const RAISED: Pose = [
    ("upperarm.r", [-70.0, -20.0, 150.0]),
    ("lowerarm.r", [0.0, 0.0, 70.0]),
];

pub(super) const POKE_DURATION_S: f32 = 0.9;
pub(super) const CAST_DURATION_S: f32 = 0.8;
const POKE_KEYS: [(f32, Pose); 4] = [
    (0.0, REST),
    (0.12, EXTENDED),
    (0.62, EXTENDED),
    (POKE_DURATION_S, REST),
];
const CAST_KEYS: [(f32, Pose); 5] = [
    (0.0, REST),
    (0.18, RAISED),
    (0.32, EXTENDED),
    (0.55, EXTENDED),
    (CAST_DURATION_S, REST),
];

/// The arm clip drives two bones and Bevy averages it with the body's clip
/// by weight; this lets it win, as `Adventurer.tsx`'s `POKE_ARM_WEIGHT`.
const ARM_WEIGHT: f32 = 8.0;
const FADE_IN_S: f32 = 0.05;
const FADE_OUT_S: f32 = 0.14;
/// The poke's finger hides this long before the clip ends.
const FINGER_EARLY_S: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArmClip {
    Poke,
    Cast,
}

impl ArmClip {
    pub(super) fn duration(self) -> f32 {
        match self {
            ArmClip::Poke => POKE_DURATION_S,
            ArmClip::Cast => CAST_DURATION_S,
        }
    }

    pub(super) fn build(self, rig: &GltfNode, nodes: &Assets<GltfNode>) -> AnimationClip {
        match self {
            ArmClip::Poke => arm_clip(&POKE_KEYS, rig, nodes),
            ArmClip::Cast => arm_clip(&CAST_KEYS, rig, nodes),
        }
    }
}

fn arm_clip(keys: &[(f32, Pose)], rig: &GltfNode, nodes: &Assets<GltfNode>) -> AnimationClip {
    let mut clip = AnimationClip::default();
    for (i, (bone, _)) in keys[0].1.iter().enumerate() {
        let Some(target) = target_of(rig, bone, nodes) else {
            continue;
        };
        let curve = AnimatableKeyframeCurve::new(keys.iter().map(|(t, pose)| {
            let [x, y, z] = pose[i].1;
            (
                *t,
                Quat::from_euler(
                    EulerRot::XYZ,
                    x.to_radians(),
                    y.to_radians(),
                    z.to_radians(),
                ),
            )
        }))
        .expect("ordered keys");
        clip.add_curve_to_target(
            target,
            AnimatableCurve::new(animated_field!(Transform::rotation), curve),
        );
    }
    clip
}

/// The animation target of the bone `name` under `rig`, by its name path.
fn target_of(rig: &GltfNode, name: &str, nodes: &Assets<GltfNode>) -> Option<AnimationTargetId> {
    fn walk(
        node: &GltfNode,
        name: &str,
        path: &mut Vec<Name>,
        nodes: &Assets<GltfNode>,
    ) -> Option<AnimationTargetId> {
        path.push(Name::new(node.name.clone()));
        if node.name == name {
            return Some(AnimationTargetId::from_names(path.iter()));
        }
        for child in node.children.iter().filter_map(|c| nodes.get(c)) {
            if let Some(found) = walk(child, name, path, nodes) {
                return Some(found);
            }
        }
        path.pop();
        None
    }
    walk(rig, name, &mut vec![], nodes)
}

/// Starts `node` over from the top, on top of the body's clip.
pub(super) fn start(player: &mut AnimationPlayer, node: AnimationNodeIndex) {
    player
        .start(node)
        .set_repeat(RepeatAnimation::Never)
        .set_weight(0.0);
}

/// Eases a playing arm clip in and out, and stops it once done. Returns
/// whether it still plays.
pub(super) fn ease(player: &mut AnimationPlayer, node: AnimationNodeIndex, clip: ArmClip) -> bool {
    let Some(active) = player.animation_mut(node) else {
        return false;
    };
    if active.is_finished() {
        player.stop(node);
        return false;
    }
    let weight = arm_weight(active.seek_time(), clip.duration());
    active.set_weight(weight);
    true
}

/// The arm clip's weight `t` seconds in.
fn arm_weight(t: f32, duration: f32) -> f32 {
    let fade_in = (t / FADE_IN_S).clamp(0.0, 1.0);
    let fade_out = ((duration - t) / FADE_OUT_S).clamp(0.0, 1.0);
    ARM_WEIGHT * fade_in.min(fade_out)
}

/// Whether the poke's finger shows `t` seconds into the poke.
pub(super) fn finger_shown(t: f32) -> bool {
    t < POKE_DURATION_S - FINGER_EARLY_S
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> (GltfNode, Assets<GltfNode>) {
        let mut nodes = Assets::<GltfNode>::default();
        let node = |name: &str, children| GltfNode {
            index: 0,
            name: name.into(),
            children,
            mesh: None,
            skin: None,
            transform: Transform::IDENTITY,
            is_animation_root: false,
            extras: None,
        };
        let lower = nodes.add(node("lowerarm.r", vec![]));
        let upper = nodes.add(node("upperarm.r", vec![lower]));
        let chest = nodes.add(node("chest", vec![upper]));
        (node("Rig_Medium", vec![chest]), nodes)
    }

    #[test]
    fn the_arm_clips_drive_only_the_right_arm_for_their_length() {
        let (rig, nodes) = rig();
        for clip in [ArmClip::Poke, ArmClip::Cast] {
            let built = clip.build(&rig, &nodes);
            assert_eq!(built.duration(), clip.duration(), "{clip:?}");
            assert_eq!(built.curves().len(), 2, "{clip:?}");
            let upper = target_of(&rig, "upperarm.r", &nodes).unwrap();
            let names = ["Rig_Medium", "chest", "upperarm.r"].map(Name::new);
            assert_eq!(upper, AnimationTargetId::from_names(names.iter()));
        }
    }

    #[test]
    fn the_arm_eases_in_holds_and_eases_out() {
        assert_eq!(arm_weight(0.0, 0.9), 0.0);
        assert_eq!(arm_weight(0.3, 0.9), ARM_WEIGHT);
        assert!(arm_weight(0.85, 0.9) < ARM_WEIGHT);
        assert_eq!(arm_weight(0.9, 0.9), 0.0);
        assert!(finger_shown(0.5));
        assert!(!finger_shown(0.85));
    }
}
