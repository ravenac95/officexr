//! Port of `retargetClip.ts`: the shared `Rig_Medium` clips are authored on
//! the rig's skeleton, whose bones can be longer or shorter than a
//! character's. A clip's bone translations are re-expressed in the
//! character's proportions: the offset from the rig's rest translation,
//! scaled by the bone length ratio, added to the character's own rest
//! translation. Scales are rebased the same way; rotations are untouched.
//!
//! The TS version rewrites each clip per character. Bevy's clips are opaque
//! curves, so instead the same mapping is applied to each bone's
//! `Transform` after the clips have written it.

use bevy::prelude::*;

const EPSILON: f32 = 1e-6;
const HIPS: &str = "hips";

/// A character bone whose rest pose differs from the rig's.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Retarget {
    rig_translation: Vec3,
    own_translation: Vec3,
    ratio: f32,
    /// The character's rest scale over the rig's, per axis.
    scale: Vec3,
    /// What this last wrote, so a value the clips didn't rewrite isn't mapped twice.
    written: (Vec3, Vec3),
}

impl Retarget {
    /// `None` when the bone's rest pose already matches the rig's.
    /// `hips_ratio` stands in for the length ratio of a zero-length rig bone.
    pub(crate) fn new(rig: &Transform, own: &Transform, hips_ratio: f32) -> Option<Self> {
        let moved = rig.translation.distance_squared(own.translation) > EPSILON;
        let scaled = rig.scale.distance_squared(own.scale) > EPSILON;
        if !moved && !scaled {
            return None;
        }
        let rig_length = rig.translation.length();
        let ratio = if rig_length > EPSILON {
            own.translation.length() / rig_length
        } else {
            hips_ratio
        };
        let safe = |v: f32| if v.abs() > EPSILON { v } else { 1.0 };
        Some(Self {
            rig_translation: rig.translation,
            own_translation: own.translation,
            ratio,
            scale: own.scale / rig.scale.map(safe),
            written: (own.translation, own.scale),
        })
    }

    /// The bone's transform as the clips left it, mapped into the character's proportions.
    fn map(&self, translation: Vec3, scale: Vec3) -> (Vec3, Vec3) {
        (
            self.own_translation + (translation - self.rig_translation) * self.ratio,
            scale * self.scale,
        )
    }
}

/// The character's hips length over the rig's, or 1 when either is zero.
pub(crate) fn hips_ratio(rig_hips: Option<Vec3>, own_hips: Option<Vec3>) -> f32 {
    match (rig_hips, own_hips) {
        (Some(r), Some(o)) if r.length() > EPSILON && o.length() > EPSILON => {
            o.length() / r.length()
        }
        _ => 1.0,
    }
}

pub(crate) fn is_hips(name: &str) -> bool {
    name == HIPS
}

/// After the clips have animated this frame, maps each retargeted bone the
/// clips wrote. A bone they didn't write keeps what this wrote last frame.
pub(crate) fn retarget_bones(mut bones: Query<(&mut Transform, &mut Retarget)>) {
    for (mut transform, mut retarget) in &mut bones {
        let current = (transform.translation, transform.scale);
        if current == retarget.written {
            continue;
        }
        let (translation, scale) = retarget.map(current.0, current.1);
        transform.translation = translation;
        transform.scale = scale;
        retarget.written = (translation, scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bone_like_the_rigs_is_left_alone() {
        let rest = Transform::from_xyz(0.0, 0.4, 0.0);
        assert_eq!(Retarget::new(&rest, &rest, 1.0), None);
    }

    #[test]
    fn a_longer_bone_keeps_the_clips_motion_scaled_to_its_length() {
        let rig = Transform::from_xyz(0.0, 0.4, 0.0);
        let own = Transform::from_xyz(0.0, 0.6, 0.0);
        let r = Retarget::new(&rig, &own, 1.0).unwrap();
        // The rig's rest maps to the character's rest.
        assert_eq!(r.map(rig.translation, Vec3::ONE).0, own.translation);
        // A clip moving the bone 0.1 up moves the longer bone 0.15 up.
        let (t, _) = r.map(Vec3::new(0.0, 0.5, 0.0), Vec3::ONE);
        assert!(t.distance(Vec3::new(0.0, 0.75, 0.0)) < 1e-5);
    }

    #[test]
    fn scales_are_rebased_and_a_zero_length_bone_takes_the_hips_ratio() {
        let rig = Transform::from_scale(Vec3::splat(2.0));
        let own = Transform::from_xyz(0.0, 0.0, 0.1).with_scale(Vec3::ONE);
        let r = Retarget::new(&rig, &own, 1.5).unwrap();
        let (t, s) = r.map(Vec3::new(0.0, 0.0, 0.2), Vec3::splat(4.0));
        assert_eq!(s, Vec3::splat(2.0));
        assert!(t.distance(Vec3::new(0.0, 0.0, 0.4)) < 1e-5);
        assert_eq!(hips_ratio(Some(Vec3::Y), Some(Vec3::Y * 1.2)), 1.2);
        assert_eq!(hips_ratio(None, Some(Vec3::Y)), 1.0);
    }

    #[test]
    fn a_bone_the_clips_did_not_rewrite_is_not_mapped_again() {
        let mut app = App::new();
        app.add_systems(Update, retarget_bones);
        let rig = Transform::from_xyz(0.0, 0.4, 0.0);
        let own = Transform::from_xyz(0.0, 0.6, 0.0);
        let bone = app
            .world_mut()
            .spawn((own, Retarget::new(&rig, &own, 1.0).unwrap()))
            .id();
        app.update();
        let at = |app: &App| app.world().get::<Transform>(bone).unwrap().translation;
        assert_eq!(at(&app), own.translation, "the rest pose stays");
        app.world_mut().get_mut::<Transform>(bone).unwrap().translation = Vec3::Y * 0.5;
        app.update();
        app.update();
        assert!(at(&app).distance(Vec3::Y * 0.75) < 1e-5);
    }
}
