import { useState, type ReactNode } from 'react';
import type { PersistentLocalState } from '@officexr/core-refactor';
import { PlayView, simFromSearch } from '@officexr/world/react';
import type { CameraMode } from '@officexr/world/react';
import type { TopDownPlayViewProps } from '@officexr/world/top-down';
import { Switch } from '../hud/ui/Switch';
import { CameraModeControl } from './CameraModeControl';
import { CharacterAction, CharacterPane } from './CharacterPane';
import { DebugFps, type DebugFpsSample } from './DebugFps';
import { debugFpsEnabled } from './debugFpsFlag';
import { storedJoystickKind, storeJoystickKind } from './joystickKind';
import { MotionLookControl } from './MotionLookControl';
import type { SettingsAction, SettingsTab } from './Settings';
import { SettingRow, SettingsModal } from './SettingsModal';

/** Everything that reaches the Bevy renderer, so `OfficeWorld` can load it only in 3D. */
export default function PlayView3d({
  viewProps,
  local,
  settings,
  onError,
  children,
}: {
  viewProps: Omit<TopDownPlayViewProps, 'children'>;
  local: PersistentLocalState;
  /** Settings shared with the 2D view. */
  settings: { tabs: SettingsTab[]; actions: SettingsAction[] };
  onError: (message: string | null) => void;
  children: ReactNode;
}) {
  const [cameraMode, setCameraMode] = useState<CameraMode>('fixed');
  // ponytail: no Recenter; the Bevy view's motion look doesn't port it yet.
  const [motionLook, setMotionLook] = useState({ enabled: false, recenterKey: 0 });
  const [motionUnsupported, setMotionUnsupported] = useState(false);
  const [invertLook, setInvertLook] = useState(false);
  const [joystickKind, setJoystickKind] = useState(() => storedJoystickKind());
  const [showFps] = useState(() => debugFpsEnabled(window.location.search, safeLocalStorage()));
  const [fps, setFps] = useState<DebugFpsSample | null>(null);

  const controls = (
    <>
      <SettingRow label="Camera">
        <CameraModeControl value={cameraMode} onChange={setCameraMode} />
      </SettingRow>
      <SettingRow label="Motion look">
        <MotionLookControl
          enabled={motionLook.enabled}
          onEnabledChange={(enabled) => setMotionLook((m) => ({ ...m, enabled }))}
          unsupported={motionUnsupported}
          onUnsupported={() => setMotionUnsupported(true)}
        />
      </SettingRow>
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
  const actions: SettingsAction[] = [{ id: 'character', render: () => <CharacterAction /> }];

  return (
    <PlayView
      {...viewProps}
      realtime="ws"
      cameraMode={cameraMode}
      onCameraModeChange={setCameraMode}
      motionLook={motionLook}
      invertLook={invertLook}
      joystickKind={joystickKind}
      hud="player"
      sim={simFromSearch(window.location.search)}
      onFrameStats={showFps ? setFps : undefined}
    >
      <SettingsModal
        title={viewProps.title}
        tabs={[...settings.tabs, { id: 'controls', label: 'Controls', content: controls }]}
        panes={[
          { id: 'character', label: 'Character', content: <CharacterPane local={local} onSaveResult={onError} /> },
        ]}
        actions={[...settings.actions, ...actions]}
      />
      {children}
      {showFps && <DebugFps sample={fps} />}
    </PlayView>
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
