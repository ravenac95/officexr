import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Document, NodeIO } from '@gltf-transform/core';
import { describe, expect, it } from 'vitest';
import { type KindMeasurement, measureKindDocument, mergeKindMeasurement } from '../kind-dimensions.ts';

const MODELS = fileURLToPath(new URL('../../../../studio/public/models', import.meta.url));

type Dims = { width: number; height: number; depth: number };
type Box = ReturnType<typeof measureKindDocument>['localAABB'];

function boxMesh(doc: Document, half: number) {
  const buffer = doc.getRoot().listBuffers()[0] ?? doc.createBuffer();
  const position = doc
    .createAccessor()
    .setType('VEC3')
    .setBuffer(buffer)
    .setArray(new Float32Array([-half, -half, -half, half, half, half]));
  return doc.createMesh().addPrimitive(doc.createPrimitive().setAttribute('POSITION', position));
}

describe('measureKindDocument', () => {
  it('matches the committed catalog entry for a bundled kind', async () => {
    const catalog = JSON.parse(await fs.readFile(path.join(MODELS, '../../../world/world-object-kinds.json'), 'utf-8')) as {
      kinds: Array<{ id: string; scale: number; dimensions: Dims; localAABB: Box }>;
    };
    const kind = catalog.kinds.find((k) => k.id === 'colored_block_blue')!;
    const doc = await new NodeIO().read(path.join(MODELS, 'blocks/colored_block_blue.gltf'));
    const { localAABB, ...dimensions } = measureKindDocument(doc, kind.scale);
    for (const key of ['width', 'height', 'depth'] as const) {
      expect(dimensions[key]).toBeCloseTo(kind.dimensions[key], 4);
    }
    for (const end of ['min', 'max'] as const) {
      for (const axis of ['x', 'y', 'z'] as const) {
        expect(localAABB[end][axis]).toBeCloseTo(kind.localAABB[end][axis], 4);
      }
    }
  });

  it('uses only the first mesh, through its world transform and the kind scale', () => {
    const doc = new Document();
    const first = doc.createNode().setMesh(boxMesh(doc, 1)).setScale([2, 1, 1]).setTranslation([0, 10, 0]);
    const second = doc.createNode().setMesh(boxMesh(doc, 50));
    doc.createScene().addChild(first).addChild(second);

    const m = measureKindDocument(doc, 0.5);
    expect(m).toEqual({
      width: 2,
      height: 1,
      depth: 1,
      localAABB: { min: { x: -1, y: 4.5, z: -0.5 }, max: { x: 1, y: 5.5, z: 0.5 } },
    });
  });
});

describe('mergeKindMeasurement', () => {
  const box = (size: number): KindMeasurement => ({
    width: size,
    height: size,
    depth: size,
    localAABB: { min: { x: 0, y: 0, z: 0 }, max: { x: size, y: size, z: size } },
  });
  const kind = (extra: Record<string, unknown> = {}) => ({
    dimensions: { width: 2, height: 2, depth: 2 },
    localAABB: box(2).localAABB,
    ...extra,
  });

  it('overwrites an unpinned kind', () => {
    const k = kind();
    expect(mergeKindMeasurement(k, box(2.02))).toEqual({ pinnedDrift: false });
    expect(k.dimensions).toEqual({ width: 2.02, height: 2.02, depth: 2.02 });
    expect(k.localAABB).toEqual(box(2.02).localAABB);
  });

  it('leaves a pinned kind alone and reports drift', () => {
    const k = kind({ dimensionsPinned: true });
    expect(mergeKindMeasurement(k, box(2.02))).toEqual({ pinnedDrift: true });
    expect(k).toEqual(kind({ dimensionsPinned: true }));
  });

  it('reports no drift when a pinned kind matches', () => {
    expect(mergeKindMeasurement(kind({ dimensionsPinned: true }), box(2))).toEqual({ pinnedDrift: false });
  });
});
