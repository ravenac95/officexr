//! Port of `occlusion/occlusionMath.ts` and `occlusion/cutawayMath.ts`: the
//! line-of-sight, fade and roof-cutaway maths, free of Bevy's ECS.

use bevy::math::Vec3;

/// A faded occluder's opacity.
pub const FADED_OPACITY: f32 = 0.28;
/// How long a fade from opaque to [`FADED_OPACITY`] takes.
pub const FADE_SECONDS: f32 = 0.15;
/// Shorter than this, an object is a floor tile or a rug: never an occluder.
pub const FLOOR_LIKE_HEIGHT: f32 = 0.35;
/// Closes the seams between flush boxes, so a line between two can't slip through.
const BOX_GROW: f32 = 0.01;

/// How far outside a box the occupied anchor may sit and still count as in it.
pub const ANCHOR_MARGIN: f32 = 0.15;
/// A box bigger than this (a whole room) never counts as what the player is in.
pub const ANCHOR_MAX_RADIUS: f32 = 5.0;

pub const ROOF_CLEARANCE: f32 = 2.2;
pub const NAMED_ROOF_CLEARANCE: f32 = 1.9;
/// How far above the head the roof probe looks.
pub const ROOF_REACH: f32 = 60.0;
pub const CUTAWAY_RADIUS: f32 = 14.0;
pub const CUT_PLANE_SLACK: f32 = 0.02;

/// The fraction of `a → b` at which it enters the box, or `None` when it
/// misses. A viewer inside the box is blocked at once (`Some(0)`); a target
/// inside it is not blocked, so the box a player stands in is never cut.
pub fn segment_box_entry(a: Vec3, b: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let min = min - BOX_GROW;
    let max = max + BOX_GROW;
    let inside = |p: Vec3| p.cmpgt(min).all() && p.cmplt(max).all();
    if inside(a) {
        return Some(0.0);
    }
    if inside(b) {
        return None;
    }
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for axis in 0..3 {
        let (a, d, lo, hi) = (a[axis], d[axis], min[axis], max[axis]);
        if d.abs() < 1e-9 {
            if a <= lo || a >= hi {
                return None;
            }
            continue;
        }
        let (mut near, mut far) = ((lo - a) / d, (hi - a) / d);
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

pub fn is_floor_like(height_m: f32) -> bool {
    height_m < FLOOR_LIKE_HEIGHT
}

/// The opacity an occluder fades toward: gone when cut away, faded when in
/// the line of sight, else opaque.
pub fn fade_target(faded: bool, cut: bool) -> f32 {
    if cut {
        0.0
    } else if faded {
        FADED_OPACITY
    } else {
        1.0
    }
}

/// `current` moved toward `target` at the rate that fades opaque to
/// [`FADED_OPACITY`] in [`FADE_SECONDS`], landing on it exactly.
pub fn step_toward(current: f32, target: f32, dt_secs: f32) -> f32 {
    let step = (1.0 - FADED_OPACITY) / FADE_SECONDS * dt_secs;
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

/// Matches `roof`, `ceiling`, `Roof_Tile_A` and the like, case-blind.
pub fn is_roof_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("roof") || name.contains("ceiling")
}

pub fn roof_clearance(named: bool) -> f32 {
    if named {
        NAMED_ROOF_CLEARANCE
    } else {
        ROOF_CLEARANCE
    }
}

/// Whether an object whose underside is at `bottom_y` is cover over feet at `feet_y`.
pub fn is_overhead_cover(bottom_y: f32, feet_y: f32, named: bool) -> bool {
    bottom_y >= feet_y + roof_clearance(named)
}

pub fn is_above_cut_plane(bottom_y: f32, cut_y: f32) -> bool {
    bottom_y >= cut_y - CUT_PLANE_SLACK
}

/// Whether an object `(dx, dz)` from the player, `object_radius` across, is near enough to cut away.
pub fn within_cutaway_radius(dx: f32, dz: f32, object_radius: f32) -> bool {
    let r = CUTAWAY_RADIUS + object_radius;
    dx * dx + dz * dz <= r * r
}

#[cfg(test)]
mod tests {
    use super::*;

    // The expected values are `occlusionMath.test.ts`'s and `cutawayMath.test.ts`'s.
    const WALL_MIN: Vec3 = Vec3::new(-1.0, 0.0, -0.2);
    const WALL_MAX: Vec3 = Vec3::new(1.0, 3.0, 0.2);

    fn entry(a: [f32; 3], b: [f32; 3]) -> Option<f32> {
        segment_box_entry(a.into(), b.into(), WALL_MIN, WALL_MAX)
    }

    #[test]
    fn a_wall_between_camera_and_player_blocks_it() {
        let t = entry([0.0, 1.5, 5.0], [0.0, 1.5, -5.0]).unwrap();
        assert!((t - 0.48).abs() < 0.005);
    }

    #[test]
    fn a_wall_beside_or_behind_the_line_of_sight_does_not() {
        assert_eq!(entry([4.0, 1.5, 5.0], [4.0, 1.5, -5.0]), None);
        assert_eq!(entry([0.0, 1.5, 5.0], [0.0, 1.5, 1.0]), None);
    }

    #[test]
    fn a_viewer_inside_blocks_at_once_and_a_target_inside_never_does() {
        assert_eq!(entry([0.0, 1.0, 0.0], [0.0, 1.5, 5.0]), Some(0.0));
        assert_eq!(entry([0.0, 1.5, 5.0], [0.0, 1.0, 0.0]), None);
    }

    #[test]
    fn a_low_prop_under_a_high_line_of_sight_does_not_block() {
        let low_max = Vec3::new(1.0, 0.5, 0.2);
        let hit = segment_box_entry(
            Vec3::new(0.0, 3.0, 5.0),
            Vec3::new(0.0, 1.5, -5.0),
            WALL_MIN,
            low_max,
        );
        assert_eq!(hit, None);
    }

    #[test]
    fn thin_slabs_are_floor_like_and_walls_are_solid() {
        assert!(is_floor_like(0.2));
        assert!(!is_floor_like(2.0));
    }

    #[test]
    fn a_fade_takes_about_150_ms_and_lands_exactly() {
        let mut v = 1.0;
        let mut t = 0.0;
        while t < FADE_SECONDS {
            v = step_toward(v, fade_target(true, false), 0.01);
            t += 0.01;
        }
        assert!((v - FADED_OPACITY).abs() < 0.05);
        assert_eq!(step_toward(v, FADED_OPACITY, 1.0), FADED_OPACITY);
        assert_eq!(step_toward(FADED_OPACITY, fade_target(false, false), 1.0), 1.0);
        assert_eq!(step_toward(1.0, fade_target(true, true), 1.0), 0.0);
    }

    #[test]
    fn only_objects_well_above_the_head_are_cover() {
        assert!(is_overhead_cover(3.0, 0.0, false));
        assert!(!is_overhead_cover(1.5, 0.0, false));
        assert!(!is_overhead_cover(2.5, 1.0, false));
    }

    #[test]
    fn a_roof_or_ceiling_name_lowers_the_clearance() {
        assert!(!is_overhead_cover(2.0, 0.0, false));
        assert!(is_overhead_cover(2.0, 0.0, true));
        assert!(is_roof_name("Roof_Tile_A"));
        assert!(is_roof_name("office_ceiling"));
        assert!(!is_roof_name("wall_01"));
    }

    #[test]
    fn everything_at_or_above_the_lowest_cover_is_cut() {
        assert!(is_above_cut_plane(3.0, 3.0));
        assert!(is_above_cut_plane(6.0, 3.0));
        assert!(!is_above_cut_plane(2.5, 3.0));
    }

    #[test]
    fn the_cutaway_is_bounded_to_the_area_around_the_player() {
        assert!(within_cutaway_radius(CUTAWAY_RADIUS - 1.0, 0.0, 0.0));
        assert!(!within_cutaway_radius(CUTAWAY_RADIUS + 1.0, 0.0, 0.0));
        assert!(within_cutaway_radius(CUTAWAY_RADIUS + 1.0, 0.0, 2.0));
    }
}
