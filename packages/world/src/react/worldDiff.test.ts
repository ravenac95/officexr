import { describe, expect, it } from 'vitest';
import { DEFAULT_WORLD_CONSTANTS, type ObjectInstance } from '@officexr/sdk';
import { diffWorld, type WorldInputs } from './worldDiff.ts';

const block = (id: string, x = 0): ObjectInstance => ({ id, sourceCommandId: 'c', kindId: 'block', position: [x, 0, 0] });

const inputs = (instances: ObjectInstance[], over: Partial<WorldInputs> = {}): WorldInputs => ({
  instances,
  kinds: KINDS,
  lighting: DEFAULT_WORLD_CONSTANTS.lighting,
  background: DEFAULT_WORLD_CONSTANTS.background,
  bakedLayouts: [],
  ...over,
});

const KINDS = [{ id: 'block' }];

describe('diffWorld', () => {
  it("sends a new view everything, as a view's first diff needs", () => {
    const { diff } = diffWorld(null, inputs([block('a')]));
    expect(diff).toEqual({
      objects: { instances: [block('a')] },
      kinds: KINDS,
      lighting: DEFAULT_WORLD_CONSTANTS.lighting,
      background: DEFAULT_WORLD_CONSTANTS.background,
      bakedLayouts: [],
    });
  });

  it('sends nothing when nothing changed, even for recompiled instances', () => {
    const { sent } = diffWorld(null, inputs([block('a')]));
    expect(diffWorld(sent, inputs([block('a')])).diff).toBeNull();
  });

  it('upserts moved and new instances and removes gone ones', () => {
    const { sent } = diffWorld(null, inputs([block('a'), block('b'), block('c')]));
    const { diff } = diffWorld(sent, inputs([block('a'), block('b', 4), block('d')]));
    expect(diff).toEqual({ objects: { instances: [block('b', 4), block('d')] }, remove: ['c'] });
  });

  it('resends only the fields that changed', () => {
    const { sent } = diffWorld(null, inputs([block('a')]));
    const lighting = { ...DEFAULT_WORLD_CONSTANTS.lighting, sunIntensity: 0.5 };
    const bakedLayouts = [{ gltfPath: 'https://x/lobby.glb', position: [0, 0, 0] as [number, number, number], rotationY: 1 }];
    expect(diffWorld(sent, inputs([block('a')], { lighting, bakedLayouts })).diff).toEqual({ lighting, bakedLayouts });
  });
});
