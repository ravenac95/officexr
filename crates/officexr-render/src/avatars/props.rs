//! What an avatar carries or rides: `Adventurer.tsx`'s held wand and poke
//! finger on the right hand, and `Players.tsx`'s `PlayerVehicle` under the rider.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::lighting::{HemisphereFill, ObjectMaterial};
use crate::scene::LightingConfig;

/// The right hand bone the wand and finger hang from.
pub(super) const HAND: &str = "hand.r";
const WAND_TIP_COLOR: Srgba = Srgba::rgb(1.0, 0.886, 0.478);
const WAND_SHAFT_COLOR: Srgba = Srgba::rgb(0.42, 0.247, 0.114);
const FINGER_COLOR: Srgba = Srgba::rgb(0.949, 0.757, 0.604);

/// A mesh an avatar carries; a spell's tint or the frozen look leaves it be.
#[derive(Component)]
pub(crate) struct AvatarProp;

/// The wand in an avatar's hand, shown while held.
#[derive(Component)]
pub(super) struct Wand;

/// The finger a poke points with.
#[derive(Component)]
pub(super) struct Finger;

/// The vehicle model under a rider.
#[derive(Component)]
pub(super) struct Vehicle;

fn material(
    materials: &mut Assets<ObjectMaterial>,
    lighting: &LightingConfig,
    base: StandardMaterial,
) -> MeshMaterial3d<ObjectMaterial> {
    MeshMaterial3d(materials.add(ObjectMaterial {
        base,
        extension: HemisphereFill::new(lighting),
    }))
}

/// Hangs a hidden wand and finger on `hand`, lying along the bone's +X, grip at its origin.
pub(super) fn attach_to_hand(
    commands: &mut Commands,
    hand: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ObjectMaterial>,
    lighting: &LightingConfig,
) {
    let along_x = Quat::from_rotation_z(-FRAC_PI_2);
    let shaft = (
        Mesh3d(meshes.add(ConicalFrustum {
            radius_top: 0.014,
            radius_bottom: 0.02,
            height: 0.42,
        })),
        material(
            materials,
            lighting,
            StandardMaterial {
                base_color: WAND_SHAFT_COLOR.into(),
                perceptual_roughness: 0.7,
                ..default()
            },
        ),
        Transform::from_xyz(0.2, 0.0, 0.0).with_rotation(along_x),
        AvatarProp,
    );
    let tip = (
        Mesh3d(meshes.add(Sphere::new(0.03))),
        material(
            materials,
            lighting,
            StandardMaterial {
                base_color: WAND_TIP_COLOR.into(),
                unlit: true,
                ..default()
            },
        ),
        Transform::from_xyz(0.43, 0.0, 0.0),
        AvatarProp,
    );
    commands.spawn((
        Wand,
        Transform::default(),
        Visibility::Hidden,
        ChildOf(hand),
        children![shaft, tip],
    ));
    commands.spawn((
        Finger,
        AvatarProp,
        Mesh3d(meshes.add(Capsule3d::new(0.035, 0.2))),
        material(
            materials,
            lighting,
            StandardMaterial {
                base_color: FINGER_COLOR.into(),
                perceptual_roughness: 0.6,
                ..default()
            },
        ),
        Transform::from_xyz(0.2, 0.0, 0.0).with_rotation(along_x),
        Visibility::Hidden,
        ChildOf(hand),
    ));
}

/// Shows `entity` when `shown`, touching it only on a change.
pub(super) fn show(visibility: &mut Visibility, shown: bool) {
    let want = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *visibility != want {
        *visibility = want;
    }
}
