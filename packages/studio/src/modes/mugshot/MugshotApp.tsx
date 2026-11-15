import React, { useCallback, useEffect, useRef, useState } from 'react';
import JSZip from 'jszip';
import { CHARACTERS, isCharacterName } from '@officexr/world';
import type { CharacterName } from '@officexr/world';
import type { BackgroundViewConfig, LightingViewConfig } from '@officexr/world/renderer';
import { createPersistentLocalState } from '@officexr/core-refactor';
import { bodyPosition, setBevyReady, useApplication, usePrimaryBevyView, type BevyView } from '@officexr/world/react';
import { downloadBlob } from '../../lib/utils.ts';
import { bevySentinels } from '../headless/BevyCanvas.tsx';
import {
  DEFAULT_MUGSHOT_BACKGROUND,
  DEFAULT_MUGSHOT_LIGHTING,
  MUGSHOT_FOV,
  MUGSHOT_PITCH_DEG,
  cubesForMode,
  mugshotCamera,
  mugshotWorld,
  type CubeMode,
} from './mugshotScene.ts';

const SELF_ID = 'mugshot-player';
const OFFICE_ID = 'mugshot';
const DEFAULT_CHARACTER: CharacterName = 'Barbarian';
const CHAR_RADIUS = 0.4;

/** The manual Y-offset slider's starting value: about where gravity settles the body root. */
const SLIDER_DEFAULT_Y = 0.5;

const DEFAULT_DISTANCE_M = 6;
const DEFAULT_CAMERA_HEIGHT = 1.7;
const DEFAULT_VIEWPORT_W = 512;
const DEFAULT_VIEWPORT_H = 512;
const DEFAULT_AZIMUTH: AzimuthDeg = 180;
const READY_POLL_MS = 50;

type AzimuthDeg = 0 | 90 | 180 | 270;

const ANGLE_NAMES: Record<AzimuthDeg, 'north' | 'east' | 'south' | 'west'> = {
  0: 'north',
  90: 'east',
  180: 'south',
  270: 'west',
};

/**
 * Parse the active character from the URL hash. Supports both
 * `#mugshot` (default character) and `#mugshot/<CharacterName>`.
 */
function readCharacterFromHash(): CharacterName {
  if (typeof window === 'undefined') return DEFAULT_CHARACTER;
  const parts = window.location.hash.replace(/^#/, '').split('/');
  const candidate = parts[1];
  if (isCharacterName(candidate)) {
    return candidate;
  }
  return DEFAULT_CHARACTER;
}

/** v2 of the mugshot export manifest. Captures rendering inputs but
 * deliberately OMITS `yOffset` — placement is gravity-driven, so the
 * manifest carries no Y at all. On manifest-load the body is dropped
 * back under gravity (manual override cleared), reproducing the
 * renderer's NATURAL settled placement. If we recorded a tuned Y, the
 * round-trip would re-apply the human's manual lift and mask any
 * underlying placement bug. The whole point of the mugshot is that
 * baselines show what the system NATURALLY produces; the Y slider in
 * the UI exists only for live diagnostic exploration. */
export interface MugshotManifest {
  schemaVersion: 2;
  character: CharacterName;
  viewportWidth: number;
  viewportHeight: number;
  exportedAt: string;
  fixedCamera: {
    azimuthDeg: number;
    pitchDeg: number;
    height: number;
    fov: number;
    distanceM: number;
  };
  lighting: LightingViewConfig;
  background: BackgroundViewConfig;
}

/**
 * Mugshot mode: a deterministic 2×2 cube scene with a paused
 * character (bind pose), tunable camera angle / distance / viewport,
 * drawn by a Bevy view on the page's primary canvas. Drives the export-for-test workflow
 * (downloads a ZIP of 8 PNGs + manifest.json).
 *
 * Placement is GRAVITY-DRIVEN: `setWorld`'s mugshot drops the
 * character onto the cubes' colliders with `officexr-sim`, the same
 * controller every character uses, and draws it where it settled —
 * never at a tuned constant. The settled body root is mirrored into
 * `window.__OFFICE_STORE__`'s self player for drivers to read.
 *
 * The Y-offset slider is a MANUAL DIAGNOSTIC OVERRIDE for a human,
 * NOT a placement mechanism officexr relies on. While untouched
 * (`manualPlacement === false`) gravity owns the body. Once the human
 * drags the slider (or a test calls `__OFFICE_MUGSHOT_SET_Y_OFFSET__`)
 * the body is pinned at the slider value instead; "Auto (gravity)"
 * drops it back under gravity.
 */
export function MugshotApp() {
  const { catalog } = useApplication();
  const [local] = useState(() =>
    createPersistentLocalState({ selfId: SELF_ID, officeId: OFFICE_ID }),
  );
  const container = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const bevy = usePrimaryBevyView(container, (e) => setError(e.message));
  const [catalogReady, setCatalogReady] = useState(false);
  const [character, setCharacter] = useState<CharacterName>(() =>
    readCharacterFromHash(),
  );

  // `yOffset` is the manual Y-override slider value; `manualPlacement`
  // gates whether it drives the body. Default false: gravity places it.
  const [yOffset, setYOffset] = useState(SLIDER_DEFAULT_Y);
  const [manualPlacement, setManualPlacement] = useState(false);
  const [azimuthDeg, setAzimuthDeg] = useState<AzimuthDeg>(DEFAULT_AZIMUTH);
  const [distanceM, setDistanceM] = useState(DEFAULT_DISTANCE_M);
  const [cameraHeight, setCameraHeight] = useState(DEFAULT_CAMERA_HEIGHT);
  const [cubeMode, setCubeMode] = useState<CubeMode>('gltf');
  const [viewportWidth, setViewportWidth] = useState(DEFAULT_VIEWPORT_W);
  const [viewportHeight, setViewportHeight] = useState(DEFAULT_VIEWPORT_H);
  const [lighting, setLighting] = useState<LightingViewConfig>(
    DEFAULT_MUGSHOT_LIGHTING,
  );
  const [background, setBackground] = useState<BackgroundViewConfig>(
    DEFAULT_MUGSHOT_BACKGROUND,
  );
  const [exporting, setExporting] = useState(false);

  // Listen for hash changes so `#mugshot/Knight` etc. live-switches.
  useEffect(() => {
    const onHashChange = () => setCharacter(readCharacterFromHash());
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, []);

  useEffect(() => {
    const w = window as unknown as { __OFFICE_STORE__?: typeof local.store };
    w.__OFFICE_STORE__ = local.store;
    return () => {
      delete w.__OFFICE_STORE__;
    };
  }, [local]);

  // The character settles on the kinds' colliders, so the scene waits for the served catalog.
  useEffect(() => {
    let cancelled = false;
    void catalog.ready().then(() => !cancelled && setCatalogReady(true));
    return () => {
      cancelled = true;
    };
  }, [catalog]);

  // Readiness, and the view for drivers that capture it.
  useEffect(() => {
    if (!bevy) return;
    const sentinels = bevySentinels();
    sentinels.__officexrBevy = bevy.wasm;
    sentinels.__officexrBevyViews = [bevy.view];
    let frame = requestAnimationFrame(function tick() {
      setBevyReady(bevy.wasm.isReady(bevy.view).ready);
      frame = requestAnimationFrame(tick);
    });
    return () => {
      cancelAnimationFrame(frame);
      delete sentinels.__officexrBevy;
      delete sentinels.__officexrBevyViews;
      delete sentinels.__officexrBevyMugshotFeetY;
    };
  }, [bevy]);

  // The scene, settled under gravity unless the slider has pinned the body.
  useEffect(() => {
    if (!bevy || !catalogReady) return;
    const world = mugshotWorld(catalog.getCatalog().kinds, cubeMode, lighting, background);
    const json = JSON.stringify(world);
    local.actions.setWorldObjects(world.objects as Parameters<typeof local.actions.setWorldObjects>[0]);
    local.actions.upsertPlayer({ id: SELF_ID, avatar: { model: character } });
    if (manualPlacement) {
      bevy.wasm.setWorld(bevy.view, json);
      delete bevySentinels().__officexrBevyMugshotFeetY;
      return;
    }
    const feetY = bevy.wasm.setWorld(bevy.view, json, character) ?? NaN;
    bevySentinels().__officexrBevyMugshotFeetY = feetY;
    const rootY = bodyPosition({ x: 0, y: feetY, z: 0 }, CHAR_RADIUS).y;
    local.actions.setSelfPosition({ x: 0, y: rootY, z: 0 }, { x: 0, y: 0, z: 0 }, 0, false);
  }, [bevy, catalogReady, catalog, local, character, cubeMode, lighting, background, manualPlacement]);

  // The slider's pin, sent on its own so dragging doesn't resend the scene.
  useEffect(() => {
    if (!bevy || !catalogReady || !manualPlacement) return;
    bevy.wasm.setPlayers(bevy.view, {
      players: [{ id: SELF_ID, character, pos: [0, yOffset, 0], yaw: 0, vel: [0, 0, 0], name: '', airborne: false }],
      charRadius: CHAR_RADIUS,
      playerSpeed: 0,
      runSpeed: 0,
      paused: true,
    });
    local.actions.setSelfPosition({ x: 0, y: yOffset, z: 0 }, { x: 0, y: 0, z: 0 }, 0, false);
  }, [bevy, catalogReady, local, character, manualPlacement, yOffset]);

  useEffect(() => {
    bevy?.wasm.setCamera(bevy.view, mugshotCamera({ azimuthDeg, distanceM, height: cameraHeight }));
  }, [bevy, azimuthDeg, distanceM, cameraHeight]);

  // Expose test hooks for the manifest-load / per-angle capture
  // comparison spec. Production code never reads these.
  const onExportRef = useRef<typeof onExport | null>(null);
  useEffect(() => {
    const win = window as unknown as {
      __OFFICE_MUGSHOT_APPLY_MANIFEST__?: (m: MugshotManifest) => void;
      __OFFICE_MUGSHOT_SET_AZIMUTH__?: (deg: AzimuthDeg) => void;
      __OFFICE_MUGSHOT_SET_Y_OFFSET__?: (y: number) => void;
      __OFFICE_MUGSHOT_SET_CUBE_MODE__?: (mode: CubeMode) => void;
      __OFFICE_MUGSHOT_SET_CAMERA_HEIGHT__?: (h: number) => void;
      __OFFICE_MUGSHOT_TRIGGER_EXPORT__?: (opts?: {
        download?: boolean;
      }) => Promise<void>;
    };
    win.__OFFICE_MUGSHOT_APPLY_MANIFEST__ = (m: MugshotManifest) => {
      setCharacter(m.character);
      // Manifests carry no Y: the compare must see gravity's natural placement.
      setManualPlacement(false);
      setDistanceM(m.fixedCamera.distanceM);
      setCameraHeight(m.fixedCamera.height);
      setViewportWidth(m.viewportWidth);
      setViewportHeight(m.viewportHeight);
      setLighting(m.lighting);
      setBackground(m.background);
      // Not azimuthDeg: the driver sets it per angle via __OFFICE_MUGSHOT_SET_AZIMUTH__.
    };
    win.__OFFICE_MUGSHOT_SET_AZIMUTH__ = (deg) => setAzimuthDeg(deg);
    // Driving the Y override pins the body, as the human grabbing the slider does.
    win.__OFFICE_MUGSHOT_SET_Y_OFFSET__ = (y) => {
      setManualPlacement(true);
      setYOffset(y);
    };
    win.__OFFICE_MUGSHOT_SET_CUBE_MODE__ = (mode) => setCubeMode(mode);
    win.__OFFICE_MUGSHOT_SET_CAMERA_HEIGHT__ = (h) => setCameraHeight(h);
    win.__OFFICE_MUGSHOT_TRIGGER_EXPORT__ = (opts) =>
      onExportRef.current?.(opts) ?? Promise.resolve();
    return () => {
      delete win.__OFFICE_MUGSHOT_APPLY_MANIFEST__;
      delete win.__OFFICE_MUGSHOT_SET_AZIMUTH__;
      delete win.__OFFICE_MUGSHOT_SET_Y_OFFSET__;
      delete win.__OFFICE_MUGSHOT_SET_CUBE_MODE__;
      delete win.__OFFICE_MUGSHOT_SET_CAMERA_HEIGHT__;
      delete win.__OFFICE_MUGSHOT_TRIGGER_EXPORT__;
    };
  }, []);

  const onExport = useCallback(async (opts?: { download?: boolean }) => {
    if (exporting || !bevy) return;
    setExporting(true);
    const azimuthBefore = azimuthDeg;
    const cubeModeBefore = cubeMode;
    // Records the body's y at each capture, so a regression test sees an
    // export that snapped Y away from what the user tuned.
    const capturedYs: Array<{ mode: string; angle: number; y: number }> = [];
    try {
      // The PNGs keep whatever Y the user tuned; only the manifest leaves it out.
      const zip = new JSZip();
      const angles: AzimuthDeg[] = [0, 90, 180, 270];
      // Both cube modes, so the ZIP carries the GLB / primitive A/B pair under `ideal/`.
      for (const mode of ['gltf', 'primitive'] as const) {
        setCubeMode(mode);
        await waitFrames(2);
        // A capture resolves on a drawn frame, so readiness is read after Bevy applied the new cubes.
        await bevy.wasm.captureView(bevy.view);
        await waitReady(bevy);
        for (const angle of angles) {
          setAzimuthDeg(angle);
          // React commits the angle and its effect sends the camera before the capture is asked for.
          await waitFrames(2);
          const p = local.store.getState().players[SELF_ID];
          if (p) capturedYs.push({ mode, angle, y: p.pos.y });
          const png = await sizedPng(await bevy.wasm.captureView(bevy.view), viewportWidth, viewportHeight);
          const fileName =
            mode === 'gltf'
              ? `${ANGLE_NAMES[angle]}.png`
              : `primitive-${ANGLE_NAMES[angle]}.png`;
          zip.file(`ideal/${fileName}`, png);
        }
      }
      const manifest: MugshotManifest = {
        schemaVersion: 2,
        character: 'Barbarian', // export is Barbarian-only per spec
        viewportWidth,
        viewportHeight,
        exportedAt: new Date().toISOString(),
        fixedCamera: {
          azimuthDeg,
          pitchDeg: MUGSHOT_PITCH_DEG,
          height: cameraHeight,
          fov: MUGSHOT_FOV,
          distanceM,
        },
        lighting,
        background,
      };
      zip.file('manifest.json', JSON.stringify(manifest, null, 2));
      const blob = await zip.generateAsync({ type: 'blob' });
      const download = opts?.download !== false;
      if (download) {
        const stamp = new Date().toISOString().replace(/[:.]/g, '-');
        downloadBlob(blob, `mugshot-Barbarian-${stamp}.zip`);
      }
      (
        window as unknown as {
          __OFFICE_MUGSHOT_LAST_EXPORT__?: {
            capturedYs: Array<{ mode: string; angle: number; y: number }>;
            blobSize: number;
            downloaded: boolean;
          };
        }
      ).__OFFICE_MUGSHOT_LAST_EXPORT__ = {
        capturedYs,
        blobSize: blob.size,
        downloaded: download,
      };
    } catch (err) {
      console.warn('[mugshot] export failed:', err);
    } finally {
      // Y was never changed by the export, so only these two are restored.
      setAzimuthDeg(azimuthBefore);
      setCubeMode(cubeModeBefore);
      setExporting(false);
    }
  }, [
    bevy,
    azimuthDeg,
    distanceM,
    cameraHeight,
    cubeMode,
    viewportWidth,
    viewportHeight,
    local,
    lighting,
    background,
    exporting,
  ]);

  // The window hook above captured onExport from mount; keep it on the latest closure.
  useEffect(() => {
    onExportRef.current = onExport;
  }, [onExport]);

  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        minWidth: 0,
        minHeight: 0,
        background: '#0a0a0a',
      }}
    >
      <main
        style={{
          flex: 1,
          minWidth: 0,
          minHeight: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          padding: 16,
          overflow: 'auto',
        }}
      >
        <div
          // The view's canvas fills this fixed-pixel box; exports are scaled to it.
          style={{
            width: viewportWidth,
            height: viewportHeight,
            position: 'relative',
            border: '1px solid #262626',
            background: '#000',
            flexShrink: 0,
          }}
        >
          <div ref={container} style={{ position: 'absolute', inset: 0 }} />
          {error && (
            <p role="alert" style={{ position: 'absolute', inset: 0, margin: 0, padding: 8, color: '#fca5a5', font: '12px sans-serif' }}>
              The 3D view couldn't start: {error}
            </p>
          )}
        </div>
      </main>
      <ControlPanel
        character={character}
        onCharacterChange={(c) => {
          window.location.hash = `mugshot/${c}`;
        }}
        yOffset={yOffset}
        // Grabbing the slider enters manual placement (body pinned): the human's diagnostic override.
        onYOffsetChange={(v) => {
          setManualPlacement(true);
          setYOffset(v);
        }}
        manualPlacement={manualPlacement}
        onResetToGravity={() => setManualPlacement(false)}
        azimuthDeg={azimuthDeg}
        onAzimuthChange={setAzimuthDeg}
        distanceM={distanceM}
        onDistanceChange={setDistanceM}
        cameraHeight={cameraHeight}
        onCameraHeightChange={setCameraHeight}
        viewportWidth={viewportWidth}
        onViewportWidthChange={setViewportWidth}
        viewportHeight={viewportHeight}
        onViewportHeightChange={setViewportHeight}
        ambientFillIntensity={lighting.ambientFillIntensity ?? 0}
        onAmbientFillChange={(v) =>
          setLighting((prev) => ({ ...prev, ambientFillIntensity: v }))
        }
        cubeMode={cubeMode}
        onCubeModeChange={setCubeMode}
        onExport={onExport}
        exporting={exporting}
      />
    </div>
  );
}

function waitFrames(n: number): Promise<void> {
  return new Promise((resolve) => {
    let i = 0;
    const tick = () => {
      i += 1;
      if (i >= n) resolve();
      else requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

async function waitReady({ wasm, view }: BevyView): Promise<void> {
  while (!wasm.isReady(view).ready) {
    await new Promise((resolve) => setTimeout(resolve, READY_POLL_MS));
  }
}

/** `png` at `width`×`height`: a view captures at the device's pixel ratio. */
async function sizedPng(png: Blob, width: number, height: number): Promise<Blob> {
  const bitmap = await createImageBitmap(png);
  if (bitmap.width === width && bitmap.height === height) {
    bitmap.close();
    return png;
  }
  const canvas = new OffscreenCanvas(width, height);
  canvas.getContext('2d')!.drawImage(bitmap, 0, 0, width, height);
  bitmap.close();
  return canvas.convertToBlob({ type: 'image/png' });
}

interface ControlPanelProps {
  character: CharacterName;
  onCharacterChange: (c: CharacterName) => void;
  yOffset: number;
  onYOffsetChange: (v: number) => void;
  /** True when the slider has been grabbed → gravity is suspended and
   * the body is pinned at `yOffset`. False → gravity owns placement. */
  manualPlacement: boolean;
  /** Re-engage gravity placement (drop the manual pin). */
  onResetToGravity: () => void;
  azimuthDeg: AzimuthDeg;
  onAzimuthChange: (v: AzimuthDeg) => void;
  distanceM: number;
  onDistanceChange: (v: number) => void;
  cameraHeight: number;
  onCameraHeightChange: (v: number) => void;
  viewportWidth: number;
  onViewportWidthChange: (v: number) => void;
  viewportHeight: number;
  onViewportHeightChange: (v: number) => void;
  ambientFillIntensity: number;
  onAmbientFillChange: (v: number) => void;
  cubeMode: CubeMode;
  onCubeModeChange: (m: CubeMode) => void;
  onExport: () => void;
  exporting: boolean;
}

function ControlPanel(props: ControlPanelProps) {
  return (
    <aside
      data-studio-panel
      style={{
        width: 280,
        flexShrink: 0,
        background: '#111',
        borderLeft: '1px solid #262626',
        color: '#fafafa',
        font: '12px system-ui, sans-serif',
        padding: 16,
        display: 'flex',
        flexDirection: 'column',
        gap: 16,
        overflowY: 'auto',
      }}
    >
      <Section title="Character">
        <select
          value={props.character}
          onChange={(e) => props.onCharacterChange(e.target.value as CharacterName)}
          style={selectStyle}
        >
          {CHARACTERS.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </Section>

      <Section
        title={
          props.manualPlacement
            ? `Y offset · ${props.yOffset.toFixed(2)} m · MANUAL`
            : `Y offset · gravity-placed (drag to override)`
        }
      >
        <input
          type="range"
          min={0}
          max={5.0}
          step={0.01}
          value={props.yOffset}
          onChange={(e) => props.onYOffsetChange(Number(e.target.value))}
          style={rangeStyle}
        />
        {/* Manual override is a human DIAGNOSTIC only. officexr never
            relies on this offset — gravity places the character. The
            slider lets a human pin the body to inspect placement when
            the gravity render looks wrong; this button drops the pin
            and lets the body re-settle under gravity. */}
        <button
          type="button"
          onClick={props.onResetToGravity}
          disabled={!props.manualPlacement}
          style={{
            marginTop: 6,
            width: '100%',
            padding: '6px 0',
            border: 0,
            borderRadius: 3,
            cursor: props.manualPlacement ? 'pointer' : 'default',
            background: props.manualPlacement ? '#1f2937' : '#141a23',
            color: props.manualPlacement ? '#cbd5e1' : '#4b5563',
            fontSize: 11,
            textTransform: 'uppercase',
            letterSpacing: '0.04em',
          }}
        >
          ↺ Auto (gravity)
        </button>
      </Section>

      <Section title={`Camera distance · ${props.distanceM.toFixed(1)} m`}>
        <input
          type="range"
          min={2}
          max={20}
          step={0.1}
          value={props.distanceM}
          onChange={(e) => props.onDistanceChange(Number(e.target.value))}
          style={rangeStyle}
        />
      </Section>

      <Section title={`Camera height · ${props.cameraHeight.toFixed(2)} m`}>
        <input
          type="range"
          min={0.5}
          max={5}
          step={0.05}
          value={props.cameraHeight}
          onChange={(e) => props.onCameraHeightChange(Number(e.target.value))}
          style={rangeStyle}
        />
      </Section>

      <Section title={`Ambient fill · ${props.ambientFillIntensity.toFixed(2)}`}>
        <input
          type="range"
          min={0}
          max={2}
          step={0.05}
          value={props.ambientFillIntensity}
          onChange={(e) => props.onAmbientFillChange(Number(e.target.value))}
          style={rangeStyle}
        />
      </Section>

      <Section title="Camera angle">
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 4 }}>
          {([0, 90, 180, 270] as AzimuthDeg[]).map((deg) => (
            <button
              key={deg}
              type="button"
              onClick={() => props.onAzimuthChange(deg)}
              style={{
                padding: '6px 0',
                border: 0,
                borderRadius: 3,
                cursor: 'pointer',
                background: props.azimuthDeg === deg ? '#3b82f6' : '#1f2937',
                color: props.azimuthDeg === deg ? '#fff' : '#cbd5e1',
                fontSize: 11,
                fontWeight: props.azimuthDeg === deg ? 600 : 400,
                textTransform: 'uppercase',
              }}
            >
              {ANGLE_NAMES[deg][0]}
            </button>
          ))}
        </div>
      </Section>

      <Section title="Cubes">
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 4 }}>
          {(['gltf', 'primitive'] as const).map((mode) => (
            <button
              key={mode}
              type="button"
              onClick={() => props.onCubeModeChange(mode)}
              style={{
                padding: '6px 0',
                border: 0,
                borderRadius: 3,
                cursor: 'pointer',
                background: props.cubeMode === mode ? '#3b82f6' : '#1f2937',
                color: props.cubeMode === mode ? '#fff' : '#cbd5e1',
                fontSize: 11,
                fontWeight: props.cubeMode === mode ? 600 : 400,
                textTransform: 'uppercase',
              }}
            >
              {mode === 'gltf' ? 'GLB' : 'Primitive'}
            </button>
          ))}
        </div>
      </Section>

      <Section title="Viewport (px)">
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <input
            type="number"
            min={64}
            max={2048}
            step={1}
            value={props.viewportWidth}
            onChange={(e) => props.onViewportWidthChange(Number(e.target.value) || 1)}
            style={numberStyle}
          />
          <span style={{ opacity: 0.6 }}>×</span>
          <input
            type="number"
            min={64}
            max={2048}
            step={1}
            value={props.viewportHeight}
            onChange={(e) => props.onViewportHeightChange(Number(e.target.value) || 1)}
            style={numberStyle}
          />
        </div>
      </Section>

      <div style={{ flex: 1 }} />

      <button
        type="button"
        onClick={props.onExport}
        disabled={props.exporting}
        style={{
          padding: '10px 16px',
          border: 0,
          borderRadius: 4,
          background: props.exporting ? '#1f2937' : '#16a34a',
          color: '#fff',
          fontWeight: 600,
          fontSize: 13,
          cursor: props.exporting ? 'wait' : 'pointer',
        }}
      >
        {props.exporting ? 'Exporting…' : 'Export for test (Barbarian)'}
      </button>
    </aside>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div
        style={{
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '0.06em',
          opacity: 0.7,
          marginBottom: 6,
        }}
      >
        {title}
      </div>
      {children}
    </div>
  );
}

const selectStyle: React.CSSProperties = {
  width: '100%',
  padding: '6px 8px',
  background: '#1f2937',
  color: '#fafafa',
  border: '1px solid #374151',
  borderRadius: 3,
  fontSize: 12,
};

const rangeStyle: React.CSSProperties = {
  width: '100%',
};

const numberStyle: React.CSSProperties = {
  flex: 1,
  padding: '4px 6px',
  background: '#1f2937',
  color: '#fafafa',
  border: '1px solid #374151',
  borderRadius: 3,
  fontSize: 12,
  width: 0, // let flex grow
};
