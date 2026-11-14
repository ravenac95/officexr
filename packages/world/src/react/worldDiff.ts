import type { BackgroundConstants, LightingConstants, ObjectInstance } from '@officexr/sdk';
import type { WorldDiff } from '@officexr/render-wasm';

export type BakedLayoutDiff = NonNullable<WorldDiff['bakedLayouts']>[number];

/** What a view should draw. Bevy places instances at the fixed `VOXEL_SIZE`, so no cube size travels. */
export interface WorldInputs {
  instances: readonly ObjectInstance[];
  kinds: readonly unknown[];
  lighting: LightingConstants;
  background: BackgroundConstants;
  bakedLayouts: readonly BakedLayoutDiff[];
}

/** What the view was last sent. */
export interface SentWorld {
  instances: ReadonlyMap<string, ObjectInstance>;
  kinds: readonly unknown[];
  lighting: LightingConstants;
  background: BackgroundConstants;
  bakedLayoutsKey: string;
}

const sameInstance = (a: ObjectInstance, b: ObjectInstance) =>
  a.kindId === b.kindId &&
  (a.yawDeg ?? 0) === (b.yawDeg ?? 0) &&
  a.position[0] === b.position[0] &&
  a.position[1] === b.position[1] &&
  a.position[2] === b.position[2];

/**
 * The `setWorld` update that brings a view from `sent` (null for a new view,
 * which the update replaces whole) to `next`, or null when nothing changed. Instances are upserted and removed
 * by id; every other field is resent whole when it changed.
 */
export function diffWorld(sent: SentWorld | null, next: WorldInputs): { diff: WorldDiff | null; sent: SentWorld } {
  const instances = new Map(next.instances.map((i) => [i.id, i]));
  const bakedLayoutsKey = JSON.stringify(next.bakedLayouts);
  const nowSent: SentWorld = {
    instances,
    kinds: next.kinds,
    lighting: next.lighting,
    background: next.background,
    bakedLayoutsKey,
  };
  // A new view starts from nothing, whatever an earlier feed left on it.
  const diff: WorldDiff = sent ? {} : { replace: true };
  const upserts = next.instances.filter((i) => {
    const prev = sent?.instances.get(i.id);
    return !prev || !sameInstance(prev, i);
  });
  if (upserts.length > 0) {
    diff.objects = { instances: upserts };
  }
  const remove = sent ? [...sent.instances.keys()].filter((id) => !instances.has(id)) : [];
  if (remove.length > 0) {
    diff.remove = remove;
  }
  if (sent?.kinds !== next.kinds) {
    diff.kinds = [...next.kinds];
  }
  if (sent?.lighting !== next.lighting) {
    diff.lighting = next.lighting;
  }
  if (sent?.background !== next.background) {
    diff.background = next.background;
  }
  if (sent?.bakedLayoutsKey !== bakedLayoutsKey) {
    diff.bakedLayouts = [...next.bakedLayouts];
  }
  return { diff: Object.keys(diff).length > 0 ? diff : null, sent: nowSent };
}
