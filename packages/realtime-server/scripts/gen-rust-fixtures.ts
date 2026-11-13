import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import {
  createInitialOfficeState,
  DEFAULT_WORLD_CONSTANTS,
  serializeOfficeState,
  ZNetEvent,
  type NetEvent,
} from '@officexr/sdk';
import { ZFrame } from '../src/protocol.ts';

const DIR = new URL('../../../crates/officexr-protocol/tests/fixtures/', import.meta.url);

const env = { v: 1 as const, actorId: 'alice', seq: 7, t: 1700000000000 };
const vec = { x: 1, y: 2, z: 3 };
const stroke = { id: 's1', authorId: 'alice', points: [{ x: 0, y: 1 }], color: '#fff', width: 2, t: 1 };
const zombies = {
  phase: 'wave' as const,
  wave: 2,
  totalKills: 3,
  hostId: 'alice',
  entities: { z1: { id: 'z1', pos: vec, hp: 5, target: null } },
  playerHealths: { alice: 100 },
};
const map = {
  gridSize: 4,
  cubeSize: 1,
  origin: { x: 0, z: 0 },
  layers: [{ kind: 'floor', cells: [{ i: 0, j: 1 }] }],
  kinds: { floor: { id: 'floor', walkable: true, appearance: { color: '#abc' } } },
};
const objects = { cubeSize: 1, instances: [{ id: 'o1', sourceCommandId: 'c1', kindId: 'k', position: [1, 2, 3] as [number, number, number], yawDeg: 90 }] };

const events: NetEvent[] = [
  { ...env, kind: 'presence:position', pos: vec, vel: vec, yaw: 0.5, isAirborne: false },
  { ...env, kind: 'presence:status', status: 'available', mic: 'muted', jitsiRoom: 'room-1' },
  { ...env, kind: 'presence:seat', instanceId: 'desk-1' },
  { ...env, kind: 'avatar:update', avatar: { model: 'gnome', viewMode: '3d', colors: { hair: '#000' }, accessories: ['hat'] }, name: 'Alice' },
  { ...env, kind: 'chat:message', text: 'hi' },
  { ...env, kind: 'whiteboard:stroke', stroke },
  { ...env, kind: 'shot:hit', targetId: 'bob', dmg: 10 },
  { ...env, kind: 'ping:send', targetId: 'bob' },
  { ...env, kind: 'zombie:state', state: zombies },
  { ...env, kind: 'snapshot:request' },
  {
    ...env,
    kind: 'snapshot:offer',
    target: 'bob',
    state: serializeOfficeState(createInitialOfficeState({ selfId: 'alice', officeId: 'main' })),
    seqTable: { alice: 7 },
  },
  { ...env, kind: 'world:constants', constants: DEFAULT_WORLD_CONSTANTS },
  { ...env, kind: 'world:map', map },
  { ...env, kind: 'world:characters', configs: { gnome: { speedMultiplier: 1.5 } } },
  { ...env, kind: 'world:objects', objects },
  { ...env, kind: 'desk:claims', claims: { 'desk-1': 'alice' } },
];

const frames = [
  { t: 'hello', userId: 'alice', officeId: 'main', login: 'alice', token: 'tok' },
  { t: 'hello-ack', present: ['bob'] },
  { t: 'snapshot', events: events.slice(0, 2) },
  { t: 'broadcast', from: 'alice', event: events[0] },
  { t: 'track-presence', data: { cursor: [1, 2] } },
  { t: 'presence', joined: ['bob'], left: ['carol'] },
  { t: 'custom', kind: 'bots:set-count', payload: { count: 3 } },
];

rmSync(DIR, { recursive: true, force: true });
mkdirSync(DIR, { recursive: true });
const write = (name: string, schema: { parse: (v: unknown) => unknown }, value: unknown): void => {
  schema.parse(value);
  writeFileSync(new URL(`${name}.json`, DIR), `${JSON.stringify(value, null, 2)}\n`);
};
for (const event of events) {
  write(`event-${event.kind.replace(':', '_')}`, ZNetEvent, event);
}
for (const frame of frames) {
  write(`frame-${frame.t}`, ZFrame, frame);
}
