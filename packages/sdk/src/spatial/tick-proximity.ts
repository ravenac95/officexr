import type { PlayerId, PlayerState, ProximitySettings, CollisionSettings, VoiceZone } from '../game-state/types.ts';
import type { PairTracker, TrackerOutput } from './pair-tracker.ts';
import { playersInZones } from './voice-zone.ts';

export interface ProximityTickInput {
  players: Record<PlayerId, PlayerState>;
  /** Voice and desk zones: a player inside one talks only through the zone's call, so proximity never pairs them. */
  voiceZones: readonly VoiceZone[];
  selfId: PlayerId;
  settings: ProximitySettings & Pick<CollisionSettings, 'charRadius'>;
  nowMs: number;
  dtSec: number;
}

/** Advances the tracker one frame; its transitions come back as `events` for the caller to emit. */
export function tickProximity(
  tracker: PairTracker,
  { players, voiceZones, selfId, settings, nowMs, dtSec }: ProximityTickInput,
): TrackerOutput {
  return tracker.tick(players, {
    selfId,
    charRadius: settings.charRadius,
    proximityRadius: settings.proximityRadius,
    proximityOuterRadius: settings.proximityOuterRadius,
    enterDebounceMs: settings.proximityEnterDebounceMs,
    nowMs,
    dtSec,
    excluded: playersInZones(players, voiceZones),
  });
}
