# Character placement: how a character ends up standing on a cube

Read this when a character renders with its feet inside (or floating
above) the floor, or when you add a character rig.

## The chain

World Y of a character's visible feet is the sum of:

```
world feet Y
  = body root Y              (the sim: Rapier character controller)
  + BODY_Y - charRadius      (officexr-render `Players::feet`)
  + bind-pose minimum mesh Y (built into the GLB; must be ≈ 0)
```

1. **Body root Y — the sim.** `SimCore` (TS or `officexr-sim`) moves a
   kinematic body whose ball collider's bottom sits `BODY_Y - charRadius`
   (0.9 − 0.4 = 0.5 m) above the body root. The controller rests that
   ball on the cube top, so the root settles at `cube_top - 0.5` plus the
   controller's skin. The store's player position is this root.
2. **Feet anchor — `crates/officexr-render/src/avatars.rs`.**
   `Players::feet` adds `body_y - char_radius` back, so the avatar entity
   sits exactly on the surface the controller stands on, and rotates it by
   the player's yaw plus `AVATAR_YAW_OFFSET`.
3. **The GLB.** The model is drawn at that anchor unchanged: there is no
   per-rig foot correction, so a rig's lowest bind-pose vertex must be at
   its origin (every KayKit character is within 2 mm). A rig authored with
   its origin elsewhere floats or sinks by that offset.

The body geometry is shared by every rig, so only step 3 varies per
character.

## Checking it

- `cargo test -p officexr-render --features visual-tests` renders the
  visual scenarios and asserts feet within 10 cm of the surface (see
  CLAUDE.md). Its `character-on-surface.json` scenario stands each built-in
  character on the cubes.
- The studio Mugshot mode shows one character on a platform for a quick
  look.

## Adding a character

Export the GLB with its origin at the soles in bind pose, add it to the
character registry (`packages/world/src/characters/registry.ts`), and check
it in Mugshot mode. Fix a floating or sunk character in the GLB, not by
offsetting it in the renderer.
