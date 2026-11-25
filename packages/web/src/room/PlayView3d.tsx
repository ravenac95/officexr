import { useEffect, useState, type ReactNode } from 'react';
import type { PersistentLocalState } from '@officexr/core-refactor';
import { PlayView, simFromSearch, type CameraMode, type PlayViewProps } from '@officexr/world/react';
import { Switch } from '../hud/ui/Switch';
import { CharacterLook } from './CharacterLook';
import { DebugFps, type DebugFpsSample } from './DebugFps';
import { debugFpsEnabled } from './debugFpsFlag';
import { storedJoystickKind, storeJoystickKind } from './joystickKind';
import { MotionLookControl } from './MotionLookControl';
import { SettingRow } from '../hud/settings/parts';
import { PerfReadout } from '../perf/PerfReadout';
import { publishFrameStats } from '../perf/perfStats';
import type { FrameStatsSample } from '@officexr/world/react';
import { useShowPerformance } from '../perf/perfSetting';
import { angleHeld, cameraModeOf, forgetLegacyJoystickChoice, storedOpenView, viewOfCameraMode, type GameView, type OpenView } from './gameView';
import type { SettingsAction } from './Settings';
import { ViewPicker } from './ViewPicker';

/**
 * The 2D view: the same 3D scene through the fixed follow camera, swung due south and tilted
 * steeply down so the office reads like a floor plan (Gather-style). A long lens from high up
 * keeps walls from leaning. Wheel and pinch zoom.
 */
const OVERHEAD_CAMERA = {
  azimuthDeg: 180,
  pitchDeg: -65,
  height: 60,
  fov: 20,
  centred: true,
} as const;

const BIRDS_EYE_CAMERA = { ...OVERHEAD_CAMERA, pitchDeg: -89.9 } as const;

const ANGLED_CAMERA = { centred: true } as const;

const fixedCameraOf = (view: GameView) =>
  view === 'map' ? OVERHEAD_CAMERA : view === 'birds-eye' ? BIRDS_EYE_CAMERA : view === 'angled' ? ANGLED_CAMERA : undefined;

export interface PlayViewSettings {
  controls: ReactNode;
  motion: ReactNode;
  look: ReactNode;
  actions: SettingsAction[];
  joystickFloats: boolean;
}

/** Everything that reaches the Bevy renderer, lazily loaded by `OfficeWorld`. */
export default function PlayView3d({
  viewProps,
  overhead,
  local,
  onError,
  children,
}: {
  viewProps: Pick<
    PlayViewProps,
    'store' | 'actions' | 'rules' | 'bus' | 'sync' | 'handshake' | 'selfId' | 'mapName' | 'title' | 'loading' | 'renderLoadError' | 'storages' | 'onPingPlayer' | 'onPingFrozen' | 'placement' | 'missionMarker'
  >;
  /** The 2D view: lock the camera overhead instead of offering camera modes. */
  overhead: boolean;
  local: PersistentLocalState;
  onError: (message: string | null) => void;
  children: (settings: PlayViewSettings) => ReactNode;
}) {
  const [openView, setOpenView] = useState<OpenView>(storedOpenView);
  const view: GameView = overhead ? 'map' : openView;
  const cameraMode = cameraModeOf(view);
  const setCameraMode = (mode: CameraMode) => setOpenView((current) => viewOfCameraMode(mode, current));
  const [motionLook, setMotionLook] = useState({ enabled: false, recenterKey: 0 });
  const [motionUnsupported, setMotionUnsupported] = useState(false);
  const [invertLook, setInvertLook] = useState(false);
  useEffect(forgetLegacyJoystickChoice, []);
  const showPerformance = useShowPerformance();
  const [joystickKind, setJoystickKind] = useState(() => storedJoystickKind());
  const [showFps] = useState(() => debugFpsEnabled(window.location.search, safeLocalStorage()));
  const [fps, setFps] = useState<DebugFpsSample | null>(null);
  const onFrameStats =
    showPerformance || showFps
      ? (sample: FrameStatsSample | null) => {
          if (showPerformance) {
            publishFrameStats(sample);
          }
          if (showFps) {
            setFps(sample);
          }
        }
      : undefined;
  const lookLocked = angleHeld(view, motionLook.enabled);

  const controls = (
    <>
      <ViewPicker value={view} onOpenView={setOpenView} />
      <SettingRow label="Invert look">
        <Switch aria-label="Invert look" checked={invertLook} onCheckedChange={setInvertLook} />
      </SettingRow>
      <SettingRow label="Page joystick">
        <Switch
          aria-label="Page joystick"
          checked={joystickKind === 'dom'}
          onCheckedChange={(dom) => {
            const kind = dom ? 'dom' : 'bevy';
            storeJoystickKind(kind);
            setJoystickKind(kind);
          }}
        />
      </SettingRow>
    </>
  );
  const motion = (
    <SettingRow label="Motion look">
      <MotionLookControl
        enabled={motionLook.enabled}
        onEnabledChange={(enabled) => setMotionLook((m) => ({ ...m, enabled }))}
        onRecenter={() => setMotionLook((m) => ({ ...m, recenterKey: m.recenterKey + 1 }))}
        unsupported={motionUnsupported}
        onUnsupported={() => setMotionUnsupported(true)}
      />
    </SettingRow>
  );
  const settings: PlayViewSettings = {
    controls,
    motion,
    look: <CharacterLook local={local} onSaveResult={onError} />,
    actions: [],
    joystickFloats: lookLocked,
  };

  return (
    <>
    <PlayView
      {...viewProps}
      realtime="ws"
      cameraMode={cameraMode}
      onCameraModeChange={overhead ? undefined : setCameraMode}
      fixedCamera={fixedCameraOf(view)}
      motionLook={motionLook}
      invertLook={invertLook}
      lookLocked={lookLocked}
      joystickKind={joystickKind}
      hud="player"
      sim={simFromSearch(window.location.search)}
      onFrameStats={onFrameStats}
    >
      {children(settings)}
      {showFps && <DebugFps sample={fps} />}
    </PlayView>
    {showPerformance && <PerfReadout mode={cameraMode} />}
    </>
  );
}

/** `localStorage`, or undefined where reading it throws (a sandboxed frame). */
function safeLocalStorage(): Storage | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}
