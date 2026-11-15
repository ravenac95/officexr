/**
 * Two bots in a real Rapier world, over the in-memory channel hub: each
 * blocks the other through its peer mirror, and a bot that falls with no
 * floor beneath it respawns.
 */
import { afterEach, describe, expect, it } from 'vitest';
import { createInMemoryChannelHub, InMemoryChannel, type Vec3, type WorldObjects } from '@officexr/sdk';
import { FakeClock } from '@officexr/sdk/test-harness';
import { BotDriver } from './BotDriver.ts';
import { BotPool } from './BotPool.ts';
import { MAX_FALL_VELOCITY, pickRespawnPosition, respawnThreshold } from '../physics/rules.ts';

const DT_MS = 16;
const CHAR_RADIUS = 0.4;

/** A 1 m cube floor, x -10..10 and z -2..2, its top at y = 0. */
const FLOOR: WorldObjects = {
  cubeSize: 1,
  instances: Array.from({ length: 20 * 4 }, (_, i) => {
    const position: [number, number, number] = [(i % 20) - 10, -1, Math.floor(i / 20) - 2];
    return { id: `f${i}`, sourceCommandId: `f${i}`, kindId: 'floor', position };
  }),
};

let stops: Array<() => void> = [];
afterEach(() => {
  stops.forEach((stop) => stop());
  stops = [];
});

describe('BotDriver with two bots', () => {
  it('peer mirrors stop two bots walking head-on from passing through each other', async () => {
    const hub = createInMemoryChannelHub();
    const pool = new BotPool({
      createChannel: (botId) => new InMemoryChannel(hub, botId),
      localPlayerId: 'local-player',
      getInitialWorld: () => ({ worldObjects: FLOOR }),
    });
    stops.push(() => pool.stop());
    await pool.setCount(2);
    pool.respawnAll([
      { x: -4, y: 0.5, z: 0 },
      { x: 4, y: 0.5, z: 0 },
    ]);
    const [a, b] = pool.list();
    for (let i = 0; i < 30; i++) {
      pool.tick(DT_MS);
    }

    pool.setLinearWalkDir(0, { x: 1, z: 0 });
    pool.setLinearWalkDir(1, { x: -1, z: 0 });
    let closest = Infinity;
    // Long enough at walking speed for each to cover the whole 8 m gap alone.
    for (let i = 0; i < 250; i++) {
      pool.tick(DT_MS);
      const [pa, pb] = [a!.getBotPos(), b!.getBotPos()];
      expect(pa.x, `tick ${i}`).toBeLessThan(pb.x);
      closest = Math.min(closest, Math.hypot(pb.x - pa.x, pb.z - pa.z));
    }
    expect(closest).toBeGreaterThan(2 * CHAR_RADIUS - 0.05);
    expect(closest).toBeLessThan(2 * CHAR_RADIUS + 0.3);
  });

  it('respawns a bot falling fast with no floor beneath, before the depth backstop', async () => {
    const hub = createInMemoryChannelHub();
    const spawn: Vec3 = { x: 0, y: 100, z: 0 };
    const bot = new BotDriver({
      createChannel: (botId) => new InMemoryChannel(hub, botId),
      localPlayerId: 'local-player',
      startPos: { x: 0, y: 200, z: 0 },
      clock: new FakeClock(0),
    });
    stops.push(() => bot.stop());
    await bot.start();
    bot.setSpawnList([spawn]);
    const backstop = respawnThreshold({ cubeSize: 1, instances: [] }, [spawn]);

    const ys = [bot.getBotPos().y];
    // About twice as long as gravity takes to reach MAX_FALL_VELOCITY.
    for (let i = 0; i < 200; i++) {
      bot.tick(DT_MS);
      ys.push(bot.getBotPos().y);
    }
    const jump = ys.findIndex((y, i) => i > 0 && y > ys[i - 1]!);
    expect(jump, `never respawned falling to ${MAX_FALL_VELOCITY} m/s`).toBeGreaterThan(0);
    expect(ys[jump]).toBeCloseTo(pickRespawnPosition([spawn], 0)!.y);
    expect(ys[jump - 1]).toBeGreaterThan(backstop);
  });
});
