use rapier3d::prelude::{Group, InteractionGroups};

const BODY: u32 = 1 << 0;
const WALL: u32 = 1 << 1;
const INNER: u32 = 1 << 2;
const OUTER: u32 = 1 << 3;

const fn groups(membership: u32, filter: u32) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_retain(membership),
        Group::from_bits_retain(filter),
    )
}

/// A character's solid body: collides with bodies and walls, senses both proximity rings.
pub const BODY_GROUPS: InteractionGroups = groups(BODY, BODY | WALL | INNER | OUTER);
/// Static obstacles collide only with bodies.
pub const WALL_GROUPS: InteractionGroups = groups(WALL, BODY);
