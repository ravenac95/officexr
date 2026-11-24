//! An avatar's colours: a spell's tint and glow (`Adventurer.tsx`) and the
//! frozen look of an away claimant (`frozenLook.ts`), both through the
//! [`AvatarLook`] uniform every avatar material carries.

use bevy::prelude::*;

use super::{PlayerRender, Tint};
use crate::lighting::{AvatarLook, ObjectMaterial};

pub const FROZEN_OPACITY: f32 = 0.55;
const FROZEN_DESATURATION: f32 = 0.85;
const FROZEN_RIM: Srgba = Srgba::rgb(0.624, 0.816, 1.0);
const FROZEN_RIM_STRENGTH: f32 = 0.8;
/// The share of the tint a glowing spell adds as light.
const GLOW: f32 = 0.7;

/// What `player`'s materials should look like.
pub(super) fn look_of(player: &PlayerRender) -> AvatarLook {
    let mut look = AvatarLook::default();
    if let Some(Tint {
        color,
        strength,
        glow,
    }) = player.tint
    {
        look.tint = color.to_linear().with_alpha(strength.clamp(0.0, 1.0));
        look.glow = if glow { GLOW } else { 0.0 };
    }
    if player.frozen {
        look.desaturate = FROZEN_DESATURATION;
        look.rim = LinearRgba::from(FROZEN_RIM).with_alpha(FROZEN_RIM_STRENGTH);
    }
    look
}

/// Gives `material` the look, and makes it translucent while frozen.
pub(super) fn apply(material: &mut ObjectMaterial, look: AvatarLook, frozen: bool) {
    material.extension.irradiance.look = look;
    let base = &mut material.base;
    if frozen {
        base.alpha_mode = AlphaMode::Blend;
        base.base_color.set_alpha(FROZEN_OPACITY);
    } else if base.alpha_mode == AlphaMode::Blend {
        // ponytail: assumes the character's own materials are opaque, as every KayKit one is.
        base.alpha_mode = AlphaMode::Opaque;
        base.base_color.set_alpha(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_player_changes_nothing() {
        let plain = look_of(&PlayerRender::default());
        assert_eq!(plain, AvatarLook::default());
        assert_eq!((plain.tint.alpha, plain.rim.alpha, plain.glow), (0.0, 0.0, 0.0));
    }

    #[test]
    fn a_spell_tints_and_a_frozen_player_greys_and_rims() {
        let player = PlayerRender {
            tint: Some(Tint {
                color: Color::srgb(1.0, 0.0, 0.0),
                strength: 2.0,
                glow: true,
            }),
            frozen: true,
            ..default()
        };
        let look = look_of(&player);
        assert_eq!(look.tint, LinearRgba::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(look.glow, GLOW);
        assert_eq!(look.desaturate, FROZEN_DESATURATION);
        assert_eq!(look.rim.alpha, FROZEN_RIM_STRENGTH);

        let mut material = ObjectMaterial::default();
        apply(&mut material, look, true);
        assert_eq!(material.base.alpha_mode, AlphaMode::Blend);
        assert_eq!(material.base.base_color.alpha(), FROZEN_OPACITY);
        apply(&mut material, AvatarLook::default(), false);
        assert_eq!(material.base.alpha_mode, AlphaMode::Opaque);
        assert_eq!(material.base.base_color.alpha(), 1.0);
    }
}
