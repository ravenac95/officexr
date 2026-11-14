import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import type { ViewYaw } from '../react/viewYaw.tsx';
import { yawFromDirection } from './lookControls.ts';

const direction = new THREE.Vector3();

export function ViewYawProbe({ viewYaw }: { viewYaw: ViewYaw }) {
  useFrame(({ camera }) => {
    viewYaw.set(yawFromDirection(camera.getWorldDirection(direction)));
  });
  return null;
}
