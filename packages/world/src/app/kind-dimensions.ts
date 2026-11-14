import type { Document } from '@gltf-transform/core';

export interface KindMeasurement {
  width: number;
  height: number;
  depth: number;
  localAABB: {
    min: { x: number; y: number; z: number };
    max: { x: number; y: number; z: number };
  };
}

/**
 * Box of a model's first mesh primitive in scene traversal order, with every
 * vertex run through its node's world matrix, scaled by `scale`. Only that one
 * primitive counts, not the union of the scene: it is the geometry the renderer draws.
 */
export function measureKindDocument(doc: Document, scale: number): KindMeasurement {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  let found = false;
  for (const scene of doc.getRoot().listScenes()) {
    scene.traverse((node) => {
      const prim = found ? undefined : node.getMesh()?.listPrimitives()[0];
      const position = prim?.getAttribute('POSITION');
      if (!position) {
        return;
      }
      found = true;
      const m = node.getWorldMatrix();
      const v = [0, 0, 0];
      for (let i = 0; i < position.getCount(); i++) {
        position.getElement(i, v);
        for (let a = 0; a < 3; a++) {
          const p = m[a] * v[0] + m[4 + a] * v[1] + m[8 + a] * v[2] + m[12 + a];
          min[a] = Math.min(min[a], p * scale);
          max[a] = Math.max(max[a], p * scale);
        }
      }
    });
  }
  if (!found) {
    throw new Error('No mesh geometry in kind GLTF');
  }
  return {
    width: max[0] - min[0],
    height: max[1] - min[1],
    depth: max[2] - min[2],
    localAABB: {
      min: { x: min[0], y: min[1], z: min[2] },
      max: { x: max[0], y: max[1], z: max[2] },
    },
  };
}
