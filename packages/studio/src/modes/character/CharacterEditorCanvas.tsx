import React, { useEffect, useRef, useState } from 'react';
import { BevyPreview, isTypingTarget, type PreviewPlacement, type PreviewView } from '@officexr/world/react';
import type { AnimationState } from '@officexr/world/renderer';
import { publishFrameStats } from '../../perf/frame-stats.ts';

type Motion = Extract<AnimationState, 'idle' | 'walking' | 'running'>;

const WALK_SPEED = 3;
const RUN_SPEED = 6;
/** The camera looks at the character's chest. */
const TARGET_HEIGHT = 1;

interface CharacterEditorCanvasProps {
  /** A built-in character name or a catalog character kind id. */
  character: string;
  /** The character's GLB. */
  url: string;
  /** Animation state to preview when not in control. */
  previewState: AnimationState;
  /** When true, WASD moves the character and the camera follows it. When false, the character stays at
   * the origin. Either way the camera orbits on right-drag and zooms on the wheel. */
  inControl: boolean;
  /** Per-character animation time scales, so tuning sliders take effect live. */
  idleSpeed: number;
  walkSpeed: number;
  runSpeed: number;
}

/** The Characters editor's view: one character on the reference grid. */
export function CharacterEditorCanvas(props: CharacterEditorCanvasProps) {
  const placement = useRef<PreviewPlacement>({ pos: [0, 0, 0], yaw: 0 }).current;
  const motion = useRef<Motion>('idle');
  const [shownMotion, setShownMotion] = useState<Motion>('idle');

  // Every flip of control or character starts from the origin, idle.
  useEffect(() => {
    placement.pos = [0, 0, 0];
    placement.yaw = 0;
    motion.current = 'idle';
    setShownMotion('idle');
  }, [props.inControl, props.character, placement]);

  const onFrame = ({ wasm, view }: PreviewView, dtSec: number) => {
    // The controls hear the whole page's keys; typing in the panel mustn't walk the character.
    wasm.setKeysEnabled(view, !isTypingTarget(document.activeElement));
    if (props.inControl) {
      const intent = wasm.takeIntent(view);
      const [dx, dz] = intent.dir;
      let next: Motion = 'idle';
      if (dx !== 0 || dz !== 0) {
        const step = (intent.run ? RUN_SPEED : WALK_SPEED) * dtSec;
        placement.pos = [placement.pos[0] + dx * step, placement.pos[1], placement.pos[2] + dz * step];
        placement.yaw = Math.atan2(-dx, -dz);
        next = intent.run ? 'running' : 'walking';
      }
      if (next !== motion.current) {
        motion.current = next;
        setShownMotion(next);
      }
    }
    const [x, y, z] = placement.pos;
    wasm.setOrbit(view, { target: [x, y + TARGET_HEIGHT, z] });
  };

  const state = props.inControl ? shownMotion : props.previewState;
  const speed =
    state === 'idle' ? props.idleSpeed : state === 'walking' ? props.walkSpeed : state === 'running' ? props.runSpeed : 1;
  return (
    <BevyPreview
      subject={{ type: 'character', character: props.url, animation: { state, speed }, placement }}
      onFrame={onFrame}
      onFrameStats={publishFrameStats}
    />
  );
}
