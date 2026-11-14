import type { Document } from '@gltf-transform/core';
import { transformPoint } from './collider-scan.ts';

export interface KindMeasurement {
  width: number;
  height: number;
  depth: number;
  localAABB: {
    min: { x: number; y: number; z: number };
    max: { x: number; y: number; z: number };
  };
}

const GL_TRIANGLES = 4;

/**
 * Box of a model's first triangle primitive in default-scene traversal order, with
 * every vertex run through its node's world matrix, scaled by `scale`. Only that one
 * primitive counts, not the union of the scene: it is the geometry the renderer draws.
 */
export function measureKindDocument(doc: Document, scale: number): KindMeasurement {
  const scene = doc.getRoot().getDefaultScene() ?? doc.getRoot().listScenes()[0];
  let measurement: KindMeasurement | undefined;
  scene?.traverse((node) => {
    if (measurement) {
      return;
    }
    const position = node
      .getMesh()
      ?.listPrimitives()
      .find((p) => p.getMode() >= GL_TRIANGLES && p.getAttribute('POSITION'))
      ?.getAttribute('POSITION');
    if (!position) {
      return;
    }
    const world = node.getWorldMatrix();
    const min = [Infinity, Infinity, Infinity];
    const max = [-Infinity, -Infinity, -Infinity];
    const v = [0, 0, 0];
    for (let i = 0; i < position.getCount(); i++) {
      position.getElement(i, v);
      transformPoint(world, v[0], v[1], v[2]).forEach((p, a) => {
        min[a] = Math.min(min[a], p * scale);
        max[a] = Math.max(max[a], p * scale);
      });
    }
    measurement = {
      width: max[0] - min[0],
      height: max[1] - min[1],
      depth: max[2] - min[2],
      localAABB: {
        min: { x: min[0], y: min[1], z: min[2] },
        max: { x: max[0], y: max[1], z: max[2] },
      },
    };
  });
  if (!measurement) {
    throw new Error('No mesh geometry in kind GLTF');
  }
  return measurement;
}

const PIN_TOLERANCE = 1e-6;

/**
 * Writes a measurement into a raw catalog kind, unless the kind pins its
 * dimensions. Returns whether a pinned kind's size differs from the measurement.
 */
export function mergeKindMeasurement(
  kind: Record<string, unknown>,
  { localAABB, ...dimensions }: KindMeasurement,
): { pinnedDrift: boolean } {
  if (!kind.dimensionsPinned) {
    Object.assign(kind, { dimensions, localAABB });
    return { pinnedDrift: false };
  }
  const pinned = kind.dimensions as Record<keyof typeof dimensions, number>;
  return {
    pinnedDrift: (['width', 'height', 'depth'] as const).some((k) => Math.abs(pinned[k] - dimensions[k]) > PIN_TOLERANCE),
  };
}
