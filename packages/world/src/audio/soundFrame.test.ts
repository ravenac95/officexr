import { describe, expect, it } from 'vitest';
import { createActions, createBus, createStore, travelMs, type SpellCast, type Vec3 } from '@officexr/sdk';
import type { SoundCue } from './cues.ts';
import { SoundFrame, type SfxSink } from './soundFrame.ts';

function setup() {
  const store = createStore({ selfId: 'me', officeId: 'o' });
  const bus = createBus();
  const actions = createActions(store, bus);
  actions.upsertPlayer({ id: 'me', pos: { x: 0, y: 0, z: 0 } });
  actions.upsertPlayer({ id: 'bob', pos: { x: 3, y: 0, z: 0 } });
  const played: SoundCue[] = [];
  const listener: Vec3[] = [];
  const sfx: SfxSink = { play: (cue) => played.push(cue), tick: () => {}, setListener: (at) => listener.push(at) };
  const frame = new SoundFrame(() => store.getState(), bus, sfx);
  return { store, actions, frame, played, listener };
}

const sounds = (cues: SoundCue[]) => cues.map((c) => (c.type === 'shot' ? c.sound : `${c.sound}:${c.level > 0 ? 'on' : 'off'}`));

describe('SoundFrame', () => {
  it('listens from the self player and plays a poke from where the poker stands', () => {
    const { actions, frame, played, listener } = setup();
    frame.step(0);
    actions.receivePoke('bob', 'me', 0);
    expect(listener).toEqual([{ x: 0, y: 0, z: 0 }]);
    expect(played).toEqual([expect.objectContaining({ sound: 'poke', at: { x: 3, y: 0, z: 0 } })]);
    frame.dispose();
  });

  it("plays a fireball's cast, its crackle in flight, then its explosion where it lands", () => {
    const { actions, frame, played } = setup();
    const cast: SpellCast = { spell: 'fireball', origin: { x: 0, y: 1, z: 0 }, dir: { x: 1, y: 0, z: 0 }, distance: 4, targetId: null };
    actions.receiveSpellCast('me', cast, 1000);
    frame.step(1001);
    frame.step(1000 + travelMs('fireball', 4) + 1);
    frame.step(1000 + travelMs('fireball', 4) + 100);
    expect(sounds(played)).toEqual(['cast', 'fire-crackle:on', 'fire-crackle:off', 'fire-explosion']);
    const explosion = played.at(-1);
    expect(explosion).toMatchObject({ at: { x: 4, y: 1, z: 0 } });
    frame.dispose();
  });

  it("turns the pose changes of every player into cues, as a ride's mount", () => {
    const { actions, frame, played } = setup();
    frame.step(0);
    actions.applyRemotePose('bob', 'scooter', 10, 'rack-1');
    frame.step(16);
    expect(sounds(played)).toContain('mount');
    frame.dispose();
  });

  it('silences a bolt still in flight when disposed', () => {
    const { actions, frame, played } = setup();
    const cast: SpellCast = { spell: 'fireball', origin: { x: 0, y: 1, z: 0 }, dir: { x: 1, y: 0, z: 0 }, distance: 8, targetId: null };
    actions.receiveSpellCast('me', cast, 0);
    frame.step(1);
    frame.dispose();
    expect(sounds(played).at(-1)).toBe('fire-crackle:off');
  });
});
