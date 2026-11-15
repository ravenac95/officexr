import React, { useEffect, useEffectEvent, useRef, useState } from 'react';
import type { CameraConfig, Overlay, RendererModule, ViewId, WorldDiff } from '@officexr/render-wasm';
import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import { assertNever } from '../renderer/assertNever.ts';
import { DEFAULT_EDITOR_LIGHTING } from '../app/editor-lighting.ts';
import { BODY_Y } from '../physics/rules.ts';
import type { WorldObjectKind } from '../scenes/world-object-kinds-schema.ts';
import { bootBevy } from './bevyRenderer.ts';
import { frameStatsMeter, type FrameStatsSample } from './frameStatsMeter.ts';

type Vec3 = [number, number, number];

/** A character clip the preview can play; the renderer maps each to a clip in the rig. */
export type AnimationState =
  | 'idle'
  | 'walking'
  | 'running'
  | 'jumping'
  | 'shooting'
  | 'throwing'
  | 'hit'
  | 'sitting';

export interface PreviewAnimation {
  state: AnimationState;
  /** The clip's time scale. */
  speed: number;
}

/** Where the character stands; a driver may change it from frame to frame. */
export interface PreviewPlacement {
  /** Its feet. */
  pos: Vec3;
  yaw: number;
}

export type PreviewSubject =
  | {
      type: 'character';
      /** A built-in character's name, or the model's URL. */
      character: string;
      animation: PreviewAnimation;
      placement?: PreviewPlacement;
    }
  /** One instance at the origin; `spin` turns it slowly unless the pointer is over it. */
  | { type: 'kind'; kind: WorldObjectKind | null; spin: boolean };

export interface PreviewView {
  wasm: RendererModule;
  view: ViewId;
}

export interface BevyPreviewProps {
  subject: PreviewSubject;
  /** Off, the camera holds its starting pose and ignores input. */
  orbit?: boolean;
  /** A see-through background and no grid, for thumbnails. */
  transparent?: boolean;
  /** Each frame before the preview is fed, for callers that drive it. */
  onFrame?: (preview: PreviewView, dtSec: number) => void;
  onFrameStats?: (sample: FrameStatsSample | null) => void;
  /** The live view once it exists, and `null` when it goes. */
  onView?: (preview: PreviewView | null) => void;
  style?: React.CSSProperties;
}

const CHARACTER_LIGHTING = {
  ...DEFAULT_EDITOR_LIGHTING,
  sunPosition: [10, 20, 10] as Vec3,
  sunIntensity: 1.2,
  ambientFillIntensity: 0.7,
};

// A punchy key light so material overrides (tint, roughness, emissive) read clearly.
const KIND_LIGHTING = {
  ...DEFAULT_EDITOR_LIGHTING,
  sunPosition: [6, 12, 6] as Vec3,
  sunIntensity: 1.4,
  ambientFillIntensity: 0.55,
};

const BACKDROP = '#0a0a0a';
const SEE_THROUGH = '#00000000';
const CHAR_RADIUS = 0.4;
const VOXEL_SIZE = 0.5;
const SPIN_RAD_PER_SEC = 0.4;
const INSTANCE_ID = 'preview';
const GRID: Overlay = { tag: 'grid', kind: 'grid', y: 0, cellSize: 1, fade: 80 };

const ORBIT_LIMITS = { pan: false, minDistance: 1.5, maxDistance: 40 };

/** The character editor's: right-drags rotate, from a little below the feet to near overhead. */
const CHARACTER_CAMERA: CameraConfig = {
  orbit: {
    ...ORBIT_LIMITS,
    rotateButton: 'right',
    minElevationDeg: (-0.4 * 180) / Math.PI,
    maxElevationDeg: 90 - (0.05 * 180) / Math.PI,
  },
};

/** Left-drag orbits, with a third of a second of damping. */
const KIND_CAMERA: CameraConfig = { orbit: { ...ORBIT_LIMITS, rotateButton: 'left', dampingSecs: 0.33 } };

/** The orbit putting the camera at `eye`, looking at `target`. */
function orbitFromEye(target: Vec3, eye: Vec3) {
  const [x, y, z] = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]];
  return { target, azimuth: Math.atan2(x, z), elevation: Math.atan2(y, Math.hypot(x, z)), distance: Math.hypot(x, y, z) };
}

const CHARACTER_ORBIT = { target: [0, 1, 0] as Vec3, azimuth: 0, elevation: 0.45, distance: 5 };
const KIND_ORBIT = orbitFromEye([0, 0.5, 0], [2.5, 2, 2.5]);

/**
 * The voxel position that puts the kind's mesh origin at `(0, scale / 2, 0)`, where the object
 * preview has always framed it. `placement.rs` anchors the bounds' min corner at the voxel:
 * `localAABB`, else `dimensions` X/Z-centred and Y-bottom, else one voxel.
 */
function previewPosition(kind: WorldObjectKind): Vec3 {
  const min = kind.localAABB?.min ??
    (kind.dimensions
      ? { x: -kind.dimensions.width / 2, y: 0, z: -kind.dimensions.depth / 2 }
      : { x: -VOXEL_SIZE / 2, y: 0, z: -VOXEL_SIZE / 2 });
  return [min.x / VOXEL_SIZE, (kind.scale / 2 + min.y) / VOXEL_SIZE, min.z / VOXEL_SIZE];
}

function kindInstance(kind: WorldObjectKind, yawRad: number) {
  return { id: INSTANCE_ID, kindId: kind.id, position: previewPosition(kind), yawDeg: (yawRad * 180) / Math.PI };
}

function previewPlayers(subject: Extract<PreviewSubject, { type: 'character' }>) {
  const { pos, yaw } = subject.placement ?? { pos: [0, 0, 0], yaw: 0 };
  return {
    players: [
      {
        id: INSTANCE_ID,
        character: subject.character,
        // The body root, which the model stands `BODY_Y - charRadius` above.
        pos: [pos[0], pos[1] - (BODY_Y - CHAR_RADIUS), pos[2]],
        yaw,
        vel: [0, 0, 0],
        name: '',
        airborne: false,
        animation: subject.animation,
      },
    ],
    charRadius: CHAR_RADIUS,
    playerSpeed: 3,
    runSpeed: 6,
  };
}

/**
 * One character or kind model in a Bevy 'preview' view on its own canvas, on a dark backdrop with a
 * reference grid, under an orbit camera. Shares the page's Bevy app (`bootBevy`).
 */
export function BevyPreview(props: BevyPreviewProps) {
  const { subject, transparent = false, orbit = true } = props;
  const canvas = useRef<HTMLCanvasElement>(null);
  const [preview, setPreview] = useState<PreviewView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const announce = useEffectEvent((p: PreviewView | null) => props.onView?.(p));

  useEffect(() => {
    let cancelled = false;
    let mounted: PreviewView | null = null;
    void bootBevy().then((wasm) => {
      if (cancelled || !canvas.current) {
        return;
      }
      mounted = { wasm, view: wasm.createView(canvas.current, 'preview') };
      wasm.setCameraMode(mounted.view, 'orbit');
      setPreview(mounted);
      announce(mounted);
    }, (e: unknown) => {
      if (!cancelled) {
        setError(e instanceof Error ? e.message : String(e));
      }
    });
    return () => {
      cancelled = true;
      if (mounted) {
        announce(null);
        mounted.wasm.destroyView(mounted.view);
      }
    };
  }, []);

  const subjectType = subject.type;
  useEffect(() => {
    if (!preview) {
      return;
    }
    const { wasm, view } = preview;
    const character = subjectType === 'character';
    wasm.setCameraConfig(view, character ? CHARACTER_CAMERA : KIND_CAMERA);
    wasm.setOrbit(view, character ? CHARACTER_ORBIT : KIND_ORBIT);
  }, [preview, subjectType]);

  useEffect(() => {
    preview?.wasm.setInputEnabled(preview.view, orbit);
  }, [preview, orbit]);

  const yaw = useRef(0);
  const kind = subject.type === 'kind' ? subject.kind : null;
  useEffect(() => {
    if (!preview) {
      return;
    }
    const diff: WorldDiff = {
      replace: true,
      lighting: subjectType === 'character' ? CHARACTER_LIGHTING : KIND_LIGHTING,
      background: DEFAULT_WORLD_CONSTANTS.background,
      clearColor: transparent ? SEE_THROUGH : BACKDROP,
      kinds: kind ? [kind] : [],
      objects: { instances: kind ? [kindInstance(kind, yaw.current)] : [] },
    };
    preview.wasm.setWorld(preview.view, JSON.stringify(diff));
    preview.wasm.setOverlays(preview.view, transparent ? [] : [GRID]);
  }, [preview, subjectType, kind, transparent]);

  const hovered = useRef(false);
  /** What `setPlayers` last sent, so a still character isn't sent again every frame. */
  const sentPlayers = useRef('');
  const feed = useEffectEvent((p: PreviewView, dtSec: number) => {
    props.onFrame?.(p, dtSec);
    if (subject.type === 'character') {
      const players = previewPlayers(subject);
      const sent = JSON.stringify(players);
      if (sent !== sentPlayers.current) {
        sentPlayers.current = sent;
        p.wasm.setPlayers(p.view, players);
      }
    } else if (subject.type === 'kind') {
      if (subject.kind && subject.spin && !hovered.current) {
        yaw.current += dtSec * SPIN_RAD_PER_SEC;
        const diff: WorldDiff = { objects: { instances: [kindInstance(subject.kind, yaw.current)] } };
        p.wasm.setWorld(p.view, JSON.stringify(diff));
      }
    } else {
      assertNever(subject);
    }
  });
  const onFrameStats = useEffectEvent((sample: FrameStatsSample | null) => props.onFrameStats?.(sample));
  useEffect(() => {
    if (!preview) {
      return;
    }
    const { wasm, view } = preview;
    const meterStats = frameStatsMeter(() => wasm.frameStats(view), onFrameStats);
    let last = performance.now();
    let frame = requestAnimationFrame(function step(now: number) {
      feed(preview, (now - last) / 1000);
      last = now;
      meterStats(now);
      frame = requestAnimationFrame(step);
    });
    return () => {
      cancelAnimationFrame(frame);
      onFrameStats(null);
    };
  }, [preview]);

  return (
    <div style={{ position: 'relative', ...(props.style ?? { width: '100%', height: '100%' }) }}>
      <canvas
        ref={canvas}
        style={{ display: 'block', width: '100%', height: '100%' }}
        onPointerEnter={() => {
          hovered.current = true;
        }}
        onPointerLeave={() => {
          hovered.current = false;
        }}
      />
      {error && (
        <p role="alert" style={{ position: 'absolute', inset: 0, margin: 0, padding: 8, color: '#fca5a5', font: '12px sans-serif' }}>
          The 3D preview couldn't start: {error}
        </p>
      )}
    </div>
  );
}
