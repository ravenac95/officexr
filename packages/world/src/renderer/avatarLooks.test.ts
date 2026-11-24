import { describe, expect, it } from 'vitest';
import { createMultiClientHarness } from '@officexr/sdk/test-harness';
import { AvatarLooks } from './avatarLooks.ts';
import { EffectsTracker, BOLT_IMPACT_MS, POKE_SPARK_MS } from './effectsTracker.ts';
import { hexColor } from './hexColor.ts';

async function room() {
  const harness = createMultiClientHarness();
  const alice = await harness.add('alice');
  alice.actions.upsertPlayer({ id: 'bob', name: 'Bob', pos: { x: 0, y: 0, z: -1 } });
  return alice;
}

describe('hexColor', () => {
  it('reads hex and the hsl() that spells and accents are written in', () => {
    expect(hexColor('#ABC')).toBe('#aabbcc');
    expect(hexColor('#1c1410')).toBe('#1c1410');
    expect(hexColor('hsl(0, 100%, 50%)')).toBe('#ff0000');
    expect(hexColor('hsl(120, 100%, 25%)')).toBe('#008000');
    expect(hexColor('hsl(240 100% 50%)')).toBe('#0000ff');
    expect(hexColor('red')).toBeNull();
  });
});

describe('AvatarLooks', () => {
  it('floats a swimmer toward the water surface and tilts it face down as it strokes', async () => {
    const { bus, store, actions } = await room();
    const looks = new AvatarLooks(bus, 'alice', store, () => 0);
    actions.upsertPlayer({ id: 'bob', pose: 'swim', vel: { x: 3, y: 0, z: 0 } });
    const water = [{ id: 'pool', minX: -5, maxX: 5, minZ: -5, maxZ: 5, baseY: -1, surfaceY: 1 }];
    let look = looks.look(store.getState().players.bob!, store.getState(), water, 0, 1 / 60);
    for (let i = 1; i < 120; i++) {
      look = looks.look(store.getState().players.bob!, store.getState(), water, i * 16, 1 / 60);
    }
    expect(look.clip?.name).toBe('Running_A');
    expect(look.body.lift).toBeGreaterThan(0.5);
    expect(look.body.pitch).toBeGreaterThan(1);
    expect(look.body.forward).toBeLessThan(0);
    expect(look.vehicle).toBeNull();
  });

  it('turns a poked player toward the poker for a moment, and counts the pokes', async () => {
    const { bus, store } = await room();
    let now = 1000;
    const looks = new AvatarLooks(bus, 'alice', store, () => now);
    bus.emit({ kind: 'poke:received', fromId: 'alice', targetId: 'bob', at: now });
    const bob = store.getState().players.bob!;
    const alice = store.getState().players.alice!;
    expect(looks.yawOf(bob, now)).toBeCloseTo(Math.atan2(-(alice.pos.x - bob.pos.x), -(alice.pos.z - bob.pos.z)));
    expect(looks.pokeBumpOf('bob')?.count).toBe(1);
    expect(looks.look(alice, store.getState(), [], now, 0.016).pokes).toBe(1);
    now += 2000;
    expect(looks.yawOf(bob, now)).toBe(bob.yaw);
    looks.dispose();
  });

  it("puts a spell's tint, lift and aura on the player it hit", async () => {
    const { bus, store, actions } = await room();
    const looks = new AvatarLooks(bus, 'alice', store);
    actions.upsertPlayer({ id: 'bob', held: 'wand' });
    const state = {
      ...store.getState(),
      spellEffects: { bob: { spell: 'color-burst' as const, casterId: 'alice', startsAt: 0, endsAt: 10_000 } },
    };
    const look = looks.look(state.players.bob!, state, [], 500, 0.016);
    expect(look.wand).toBe(true);
    expect(look.tint?.color).toMatch(/^#[\da-f]{6}$/);
    expect(look.tint?.glow).toBe(true);
    expect(look.aura).toBe('sparkles');
  });
});

describe('EffectsTracker', () => {
  it('plays a spark per poke and a bolt per cast until each is done', async () => {
    const { bus, store } = await room();
    const effects = new EffectsTracker(bus, store, () => 0);
    bus.emit({ kind: 'poke:received', fromId: 'alice', targetId: null, at: 0 });
    bus.emit({
      kind: 'spell:cast',
      casterId: 'alice',
      cast: { spell: 'fireball', origin: { x: 0, y: 1, z: 0 }, dir: { x: 0, y: 0, z: -1 }, distance: 4, targetId: null },
      at: 0,
    });
    expect(effects.effects(100).map((e) => e.kind).sort()).toEqual(['bolt', 'pokeSpark']);
    // Fireballs fly at 8 m/s: 4 m lands at 500 ms.
    expect(effects.effects(700).map((e) => e.kind)).toEqual(['bolt']);
    expect(effects.effects(500 + BOLT_IMPACT_MS)).toEqual([]);
    expect(POKE_SPARK_MS).toBeLessThan(700);
    effects.dispose();
  });
});
