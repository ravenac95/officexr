import type { MeetingArea, PairState, PlayerId, PlayerState, ProximityEvent, TrackerOutput, Vec3, WorldObjects } from '@officexr/sdk';
import type {
  SimBakedColliders,
  SimCore,
  SimInput,
  SimKinds,
  SimPeer,
  SimProximityInput,
  SimStepResult,
} from './SimCore.ts';

export interface SimMap {
  worldObjects: WorldObjects;
  kinds: SimKinds;
  bakedColliders: SimBakedColliders[];
}

/** One frame: an optional teleport, then an optional peer sync, then the step. */
export interface SimTraceFrame {
  teleport?: Vec3;
  peers?: SimPeer[];
  step: SimInput;
}

/** Golden record of a `SimCore` run: `outputs[i]` is the step result of `inputs[i]`. */
export interface SimTrace {
  kind: 'movement';
  map: SimMap;
  inputs: SimTraceFrame[];
  outputs: SimStepResult[];
}

/** Plays `inputs` on a fresh `sim` over `map`, recording what each step returns. */
export function recordTrace(sim: SimCore, map: SimMap, inputs: SimTraceFrame[]): SimTrace {
  sim.setStaticColliders(map.worldObjects, map.kinds, map.bakedColliders);
  const outputs = inputs.map((frame) => {
    if (frame.teleport) {
      sim.teleport(frame.teleport);
    }
    if (frame.peers) {
      sim.syncPeers(frame.peers, frame.step.settings.charRadius);
    }
    return sim.step(frame.step);
  });
  return { kind: 'movement', map, inputs, outputs };
}

export interface ProximityTraceFrame {
  players: Record<PlayerId, PlayerState>;
  input: SimProximityInput;
}

/** `TrackerOutput` as plain data: maps as key-sorted entry lists, sets as sorted lists. */
export interface ProximityFrameOutput {
  events: ProximityEvent[];
  pairStates: Array<[string, PairState]>;
  playerStates: Array<[PlayerId, PairState]>;
  areas: Array<Omit<MeetingArea, 'members'> & { members: PlayerId[] }>;
  conversationFocus: TrackerOutput['conversationFocus'];
}

/** Golden record of `SimCore.tickProximity`: `outputs[i]` is the result of `inputs[i]`. */
export interface ProximityTrace {
  kind: 'proximity';
  inputs: ProximityTraceFrame[];
  outputs: ProximityFrameOutput[];
}

const byKey = (a: [string, unknown], b: [string, unknown]) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0);

function proximityFrameOutput(out: TrackerOutput): ProximityFrameOutput {
  return {
    events: out.events,
    pairStates: [...out.pairStates].sort(byKey),
    playerStates: [...out.playerStates].sort(byKey),
    areas: out.areas.map((a) => ({ ...a, members: [...a.members].sort() })),
    conversationFocus: out.conversationFocus,
  };
}

/** Plays `inputs` on a fresh `sim`, recording what each proximity tick returns. */
export function recordProximityTrace(sim: SimCore, inputs: ProximityTraceFrame[]): ProximityTrace {
  const outputs = inputs.map((f) => proximityFrameOutput(sim.tickProximity(f.players, f.input)));
  return { kind: 'proximity', inputs, outputs };
}
