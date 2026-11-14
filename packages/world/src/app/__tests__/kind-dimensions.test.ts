import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Document, NodeIO } from '@gltf-transform/core';
import { describe, expect, it } from 'vitest';
import { measureKindDocument } from '../kind-dimensions.ts';

const MODELS = fileURLToPath(new URL('../../../../studio/public/models', import.meta.url));

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
  it('measures a bundled kind model', async () => {
    const doc = await new NodeIO().read(path.join(MODELS, 'blocks/colored_block_blue.gltf'));
    const m = measureKindDocument(doc, 1);
    expect(m.width).toBeCloseTo(2, 4);
    expect(m.localAABB.min.y).toBeCloseTo(-1, 4);
    expect(m.localAABB.max.x).toBeCloseTo(1, 4);
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
