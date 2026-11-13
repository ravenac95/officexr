use std::collections::HashMap;

use bevy::prelude::*;
use officexr_render::{PlayerRender, Players};
use officexr_sim::{Vec3 as SimVec3, extrapolate_pos};

const CHARACTERS: [&str; 6] = [
    "Barbarian",
    "Knight",
    "Mage",
    "Ranger",
    "Rogue",
    "Rogue_Hooded",
];

#[derive(Clone, Copy)]
pub struct Position {
    pub pos: SimVec3,
    pub vel: SimVec3,
    pub yaw: f64,
    pub is_airborne: bool,
}

#[derive(Message)]
pub enum PeerUpdate {
    /// The full present list, after every (re)join.
    Present(Vec<String>),
    Left(Vec<String>),
    Position(String, Position),
    Avatar {
        id: String,
        model: Option<String>,
        name: String,
    },
}

#[derive(Default)]
struct Peer {
    position: Option<(Position, f64)>,
    model: Option<String>,
    name: String,
}

#[derive(Resource, Default)]
pub struct Peers(HashMap<String, Peer>);

pub fn update_peers(
    time: Res<Time>,
    mut updates: MessageReader<PeerUpdate>,
    mut peers: ResMut<Peers>,
    mut players: ResMut<Players>,
) {
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    for update in updates.read() {
        match update {
            PeerUpdate::Present(present) => peers.0.retain(|id, _| present.contains(id)),
            PeerUpdate::Left(left) => peers.0.retain(|id, _| !left.contains(id)),
            PeerUpdate::Position(id, position) => {
                peers.0.entry(id.clone()).or_default().position = Some((*position, now_ms));
            }
            PeerUpdate::Avatar { id, model, name } => {
                let peer = peers.0.entry(id.clone()).or_default();
                peer.model = model.clone();
                peer.name = name.clone();
            }
        }
    }
    players.players = peers
        .0
        .iter()
        .filter_map(|(id, peer)| {
            let (p, t_recv) = peer.position?;
            let pos = extrapolate_pos(p.pos, p.vel, Some(t_recv), now_ms);
            Some(PlayerRender {
                id: id.clone(),
                character: character_for(id, peer.model.as_deref()).into(),
                pos: Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
                // ponytail: yaw snaps to each update instead of easing.
                yaw: p.yaw as f32,
                vel: Vec3::new(p.vel.x as f32, p.vel.y as f32, p.vel.z as f32),
                name: if peer.name.is_empty() {
                    id.clone()
                } else {
                    peer.name.clone()
                },
                airborne: p.is_airborne,
            })
        })
        .collect();
}

// ponytail: catalog characters fall back to the hashed built-in, as `resolveCharacter` does for unknown models.
pub fn character_for(id: &str, model: Option<&str>) -> &'static str {
    if let Some(name) = CHARACTERS.iter().find(|c| Some(**c) == model) {
        return name;
    }
    let hash = id
        .encode_utf16()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    CHARACTERS[hash as usize % CHARACTERS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_models_win_and_others_hash_like_the_web() {
        assert_eq!(character_for("x", Some("Mage")), "Mage");
        // 'a' = 97 → 97 % 6 = 1, as `hashedCharacter('a')` in registry.ts.
        assert_eq!(character_for("a", Some("default")), "Knight");
    }
}
