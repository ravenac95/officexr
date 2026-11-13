import type { MeetingArea, PlayerId, PlayerState, TrackerOutput, Vec3, WorldObjects } from '@officexr/sdk';
import type { WasmSim } from '@officexr/sim-wasm';
import { extrapolatePos, type Extrapolatable } from './extrapolate.ts';
import type {
  SimBakedColliders,
  SimCore,
  SimInput,
  SimKinds,
  SimPeer,
  SimProximityInput,
  SimStepResult,
} from './SimCore.ts';

type RawTrackerOutput = Omit<TrackerOutput, 'areas'> & {
  areas: Array<Omit<MeetingArea, 'members'> & { members: PlayerId[] }>;
};

/** `SimCore` on the Rust `officexr-sim` compiled to wasm; its Rapier world lives in wasm memory. */
export class WasmSimCore implements SimCore {
  constructor(private readonly sim: WasmSim) {}

  setStaticColliders(worldObjects: WorldObjects, kinds: SimKinds, bakedColliders: readonly SimBakedColliders[]): void {
    this.sim.setStaticColliders({ worldObjects, kinds, bakedColliders });
  }

  syncPeers(peers: readonly SimPeer[], charRadius: number): void {
    this.sim.syncPeers(peers, charRadius);
  }

  step(input: SimInput): SimStepResult {
    return this.sim.step(input);
  }

  tickProximity(players: Record<PlayerId, PlayerState>, input: SimProximityInput): TrackerOutput {
    const raw: RawTrackerOutput = this.sim.tickProximity(
      Object.values(players).map((p) => ({ id: p.id, pos: p.pos })),
      input,
    );
    return {
      ...raw,
      areas: raw.areas.map((a) => ({ ...a, members: new Set(a.members) })),
    };
  }

  extrapolatePeer(peer: Extrapolatable, nowMs: number): Vec3 {
    return extrapolatePos(peer, nowMs);
  }

  position(): Vec3 {
    return this.sim.position();
  }

  teleport(pos: Vec3): void {
    this.sim.teleport(pos);
  }

  setGravityEnabled(on: boolean): void {
    this.sim.setGravityEnabled(on);
  }

  dispose(): void {
    this.sim.free();
  }
}

/** Fetches and instantiates the wasm module, then returns a `WasmSimCore` factory. */
export async function loadWasmSimCore(): Promise<(charRadius: number) => SimCore> {
  const wasm = await import('@officexr/sim-wasm');
  await wasm.default();
  return (charRadius) => new WasmSimCore(new wasm.WasmSim(charRadius));
}
