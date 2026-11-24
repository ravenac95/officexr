// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { createMultiClientHarness } from '@officexr/sdk/test-harness';
import type { MoveIntent, ProximityInput } from '@officexr/render-wasm';
import { SPELL_IDS, type SpellCast, type TrackerOutput, type Vec3 } from '@officexr/sdk';
import type { BotPool } from '../bot/BotPool.ts';
import { ActionInput } from '../input/action-input.ts';
import { InteractionStore, NO_PROMPT } from '../input/interaction.ts';
import { RIDE_STRATEGIES } from '../motion/ride.ts';
import { SWIM_STRATEGIES } from '../motion/swim.ts';
import type { SelfWorld } from '../renderer/useMotionWorld.ts';
import type { SimCore, SimInput, SimStepResult } from '../sim/SimCore.ts';
import { RoomFrame, type PlayersInput, type RoomFrameOptions, type RoomView } from './roomFrame.ts';

const STILL: MoveIntent = { dir: [0, 0], run: false, jump: false, yaw: 0, deflection: 0 };

function fakeSim(log: string[], start: Vec3) {
  let pos = start;
  const steps: SimInput[] = [];
  const sim = {
    syncPeers: () => log.push('sim.syncPeers'),
    tickProximity: (): TrackerOutput => {
      log.push('sim.tickProximity');
      return {
        events: [{ kind: 'proximity:entering', a: 'alice', b: 'bob' }],
        conversationFocus: { x: 3, y: 0, z: 4 },
        playerStates: new Map([['alice', 'entered']]),
        pairStates: new Map([
          ['alice|bob', 'entering'],
          ['bob|carol', 'entering'],
        ]),
        areas: [{ id: 'm', members: new Set(['alice', 'bob']), center: { x: 3, z: 4 }, radius: 2 }],
      } as unknown as TrackerOutput;
    },
    extrapolatePeer: (p: { pos: Vec3 }) => p.pos,
    wallDistance: (_origin: Vec3, _dir: Vec3, max: number) => Math.min(4, max),
    position: () => pos,
    teleport: (p: Vec3) => {
      pos = p;
    },
    step: (input: SimInput): SimStepResult => {
      log.push('sim.step');
      steps.push(input);
      pos = { x: pos.x + input.dir.x, y: pos.y, z: pos.z + input.dir.z };
      return {
        newPos: pos,
        broadcastVel: { x: input.dir.x, y: 0, z: input.dir.z },
        broadcastYaw: input.yaw,
        moved: input.dir.x !== 0 || input.dir.z !== 0,
        isAirborne: false,
        bumps: [],
      } as unknown as SimStepResult;
    },
  };
  return { sim: sim as unknown as SimCore, steps };
}

function fakeView(log: string[], intent: MoveIntent = STILL, bevyStick = true) {
  const players: PlayersInput[] = [];
  const inputEnabled: boolean[] = [];
  const sent: { sticks: [number, number][]; joystickKinds: string[] } = { sticks: [], joystickKinds: [] };
  const proximity: ProximityInput[] = [];
  const state = { intent };
  const view: RoomView = {
    takeIntent: () => {
      log.push('view.takeIntent');
      return state.intent;
    },
    setInputEnabled: (enabled) => {
      log.push('view.setInputEnabled');
      inputEnabled.push(enabled);
    },
    setStick: (stick) => sent.sticks.push(stick),
    setJoystickKind: (kind) => {
      sent.joystickKinds.push(kind);
      return bevyStick;
    },
    setWorld: () => log.push('view.setWorld'),
    setPlayers: (p) => {
      log.push('view.setPlayers');
      players.push(p);
    },
    setFollowTarget: () => log.push('view.setFollowTarget'),
    setConversationFocus: () => log.push('view.setConversationFocus'),
    setProximity: (p) => {
      log.push('view.setProximity');
      proximity.push(p);
    },
  };
  return { view, players, inputEnabled, sent, proximity, state };
}

/** A world with nothing to swim in, ride or pick up; tests override what they need. */
function emptyWorld(over: Partial<SelfWorld> = {}): SelfWorld {
  return { waterAt: () => null, rideNear: () => null, wandStandNear: () => null, anchorOf: () => null, ...over };
}

function fakeKeys() {
  const state = { held: new Set<string>(), clicked: false };
  return {
    state,
    keys: {
      held: () => state.held,
      takeClick: () => {
        const clicked = state.clicked;
        state.clicked = false;
        return clicked;
      },
    },
  };
}

async function setup(opts: { intent?: MoveIntent; options?: Partial<RoomFrameOptions>; bevyStick?: boolean } = {}) {
  const harness = createMultiClientHarness();
  const alice = await harness.add('alice');
  alice.actions.upsertPlayer({ id: 'bob', name: 'Bob', pos: { x: 5, y: 0, z: 5 } });
  const log: string[] = [];
  const { sim, steps } = fakeSim(log, alice.store.getState().players.alice!.pos);
  const { view, players, inputEnabled, sent, proximity, state: viewState } = fakeView(
    log,
    opts.intent,
    opts.bevyStick,
  );
  const setSelfPosition = alice.actions.setSelfPosition.bind(alice.actions);
  vi.spyOn(alice.actions, 'setSelfPosition').mockImplementation((...args) => {
    log.push('actions.setSelfPosition');
    setSelfPosition(...args);
  });
  alice.bus.on('proximity:entering', () => log.push('bus.emit'));
  const domJoystick: boolean[] = [];
  const bots = { tick: () => log.push('bots.tick') } as unknown as BotPool;
  const sentNet = { pokes: [] as (string | null)[], casts: [] as SpellCast[] };
  const sync = { sendPoke: (id: string | null) => sentNet.pokes.push(id), sendCast: (c: SpellCast) => sentNet.casts.push(c) };
  const { keys, state: keyState } = fakeKeys();
  const frame = new RoomFrame(
    {
      store: alice.store,
      actions: alice.actions,
      bus: alice.bus,
      sync,
      bots,
      selfId: 'alice',
      view,
      keys,
      onDomJoystick: (dom) => domJoystick.push(dom),
    },
    {
      sim,
      cameraMode: 'fixed',
      worldFocused: true,
      selfReady: true,
      spawnPoints: [],
      joystickKind: 'bevy',
      characterOf: (p) => `char-${p.id}`,
      kinds: [],
      bakedLayouts: [],
      motionWorld: emptyWorld(),
      ...opts.options,
    },
  );
  return {
    frame,
    log,
    steps,
    sim,
    players,
    inputEnabled,
    sent,
    domJoystick,
    proximity,
    actions: alice.actions,
    bus: alice.bus,
    store: alice.store,
    sentNet,
    /** Holds exactly `held` this frame, as `KeyboardEvent.key`s. */
    hold: (...held: string[]) => {
      keyState.held = new Set(held);
    },
    click: () => {
      keyState.clicked = true;
    },
    setIntent: (intent: MoveIntent) => {
      viewState.intent = intent;
    },
  };
}

describe('RoomFrame', () => {
  it("keeps SceneFrame's order: intent, proximity and its events, the step, bots, then the view", async () => {
    const { frame, log } = await setup({ intent: { dir: [1, 0], run: false, jump: false, yaw: 0, deflection: 0 } });

    frame.step(0.016, 1000);

    expect(log).toEqual([
      'view.setInputEnabled',
      'view.takeIntent',
      'sim.syncPeers',
      'sim.tickProximity',
      'bus.emit',
      'sim.step',
      'actions.setSelfPosition',
      'bots.tick',
      'view.setWorld',
      'view.setPlayers',
      'view.setFollowTarget',
      'view.setConversationFocus',
      'view.setProximity',
    ]);
  });

  it('sends the proximity output with the world constants\' glow tunables', async () => {
    const { frame, proximity, store } = await setup();

    frame.step(0.016, 1000);

    const { discRadius, enteredColor, sparkleSize } = store.getState().worldConstants.proximity;
    expect(proximity).toEqual([
      {
        players: { alice: 'entered' },
        areas: [{ id: 'm', center: [3, 4], radius: 2, members: ['alice', 'bob'] }],
        undrawn: {},
        bubbles: ['alice', 'bob'],
        // The fixed camera's bubbles are 5× as big.
        style: expect.objectContaining({ discRadius, enteredColor, sparkleSize: sparkleSize * 5 }),
      },
    ]);
  });

  it('sends the first-person player\'s feet, which setPlayers leaves out', async () => {
    const { frame, proximity, players, store } = await setup({ options: { cameraMode: 'first-person' } });

    frame.step(0.016, 1000);

    expect(players[0]!.players.map((p) => p.id)).not.toContain('alice');
    const { pos } = store.getState().players.alice!;
    const charRadius = store.getState().worldSettings.charRadius;
    expect(proximity[0]!.undrawn).toEqual({ alice: [pos.x, pos.y + 0.9 - charRadius, pos.z] });
  });

  it("steps the sim along the view's intent and moves the store's player", async () => {
    const { frame, steps, store } = await setup({ intent: { dir: [0, -1], run: true, jump: true, yaw: 0, deflection: 0 } });
    const before = store.getState().players.alice!.pos;
    const { playerSpeed, runSpeedMultiplier } = store.getState().worldSettings;

    frame.step(0.016, 1000);

    expect(steps[0]).toMatchObject({ dir: { x: 0, z: -1 }, jump: true, running: true, speed: playerSpeed * runSpeedMultiplier });
    expect(store.getState().players.alice!.pos.z).toBe(before.z - 1);
  });

  it('holds the player still in free-fly, where the keys fly the camera', async () => {
    const { frame, steps } = await setup({
      intent: { dir: [1, 0], run: false, jump: true, yaw: 0, deflection: 0 },
      options: { cameraMode: 'free-fly' },
    });

    frame.step(0.016, 1000);

    expect(steps[0]).toMatchObject({ dir: { x: 0, z: 0 }, jump: false });
  });

  it('holds the player until the world is ready', async () => {
    const { frame, log } = await setup({ intent: { dir: [1, 0], run: false, jump: false, yaw: 0, deflection: 0 }, options: { selfReady: false } });

    frame.step(0.016, 1000);

    expect(log).not.toContain('sim.step');
  });

  it('draws peers with name tags and the self player without, or not at all in first person', async () => {
    const { frame, players } = await setup();
    frame.step(0.016, 1000);
    expect(players[0]!.players.map((p) => [p.id, p.character, p.name])).toEqual([
      ['alice', 'char-alice', ''],
      ['bob', 'char-bob', 'Bob'],
    ]);

    frame.options = { ...frame.options, cameraMode: 'first-person' };
    frame.step(0.016, 1016);
    expect(players[1]!.players.map((p) => p.id)).toEqual(['bob']);
  });

  it('sends a seated player with their seat, and counts bumps with their push direction', async () => {
    const { frame, players, actions, bus } = await setup();
    actions.upsertPlayer({ id: 'bob', seat: 'chair-1' });
    bus.emit({ kind: 'collision:char-bump', selfId: 'bob', otherId: 'alice', normal: { x: 0, z: 1 } });
    bus.emit({ kind: 'collision:char-bump', selfId: 'bob', otherId: 'alice', normal: { x: 1, z: 0 } });

    frame.step(0.016, 1000);

    const [alice, bob] = players[0]!.players;
    expect(alice!.seat).toBeNull();
    expect(alice!.bump).toEqual({ count: 0, normal: [0, 0] });
    expect(bob!.seat).toEqual({ pos: [bob!.pos[0], bob!.pos[1], bob!.pos[2]], yaw: bob!.yaw });
    expect(bob!.bump).toEqual({ count: 2, normal: [1, 0] });
  });

  it("sends each player's character clip-speed tunables", async () => {
    const { frame, players, actions } = await setup();
    actions.upsertPlayer({ id: 'bob', avatar: { model: 'Knight' } });
    actions.setCharacterConfig('Knight', { walkAnimSpeed: 0.8, runAnimSpeed: 1.2 });

    frame.step(0.016, 1000);

    const [alice, bob] = players[0]!.players;
    expect(alice!.animSpeeds).toEqual({ idle: 1, walk: 1, run: 1 });
    expect(bob!.animSpeeds).toEqual({ idle: 1, walk: 0.8, run: 1.2 });
  });

  it('sends the world once, and again only when the store changes it', async () => {
    const { frame, log, store } = await setup();
    const worldSends = () => log.filter((l) => l === 'view.setWorld').length;

    frame.step(0.016, 1000);
    frame.step(0.016, 1016);
    expect(worldSends()).toBe(1);

    store.setState(() => ({
      worldObjects: { cubeSize: 0.5, instances: [{ id: 'a', sourceCommandId: 'c', kindId: 'block', position: [0, 0, 0] }] },
    }));
    frame.step(0.016, 1032);
    expect(worldSends()).toBe(2);
  });

  it('stands a seated player on movement instead of stepping', async () => {
    const seating = { sit: vi.fn(), stand: vi.fn(), setStandingCheck: vi.fn() };
    const { frame, log, actions } = await setup({ intent: { dir: [1, 0], run: false, jump: false, yaw: 0, deflection: 0 }, options: { seating } });
    actions.upsertPlayer({ id: 'alice', seat: 'chair-1' });

    frame.step(0.016, 1000);

    expect(seating.stand).toHaveBeenCalledOnce();
    expect(log).not.toContain('sim.step');
  });

  it("teleports the sim to a store position set from outside, and doesn't step that frame", async () => {
    const { frame, log, sim, actions } = await setup({ intent: { dir: [1, 0], run: false, jump: false, yaw: 0, deflection: 0 } });
    actions.setSelfPosition({ x: 40, y: 2, z: 40 }, { x: 0, y: 0, z: 0 }, 0, false);

    frame.step(0.016, 1000);

    expect(sim.position()).toEqual({ x: 40, y: 2, z: 40 });
    expect(log).not.toContain('sim.step');
  });

  it('zeroes the velocity once on the frame the player stops', async () => {
    const { frame, store, setIntent } = await setup({ intent: { dir: [1, 0], run: false, jump: false, yaw: 0, deflection: 0 } });
    frame.step(0.016, 1000);
    expect(store.getState().players.alice!.vel.x).toBe(1);

    setIntent(STILL);
    frame.step(0.016, 1016);

    expect(store.getState().players.alice!.vel).toEqual({ x: 0, y: 0, z: 0 });
  });

  it("walks the stick at its deflection's share of walk speed", async () => {
    const { frame, steps, store } = await setup({
      intent: { dir: [-1, 0], run: false, jump: false, yaw: Math.PI / 2, deflection: 0.5 },
    });

    frame.step(0.016, 1000);

    const { playerSpeed } = store.getState().worldSettings;
    expect(steps[0]!.speed).toBeCloseTo(playerSpeed * 0.5);
    expect(steps[0]!.running).toBe(false);
    expect(steps[0]!.dir).toEqual({ x: -1, z: 0 });
  });

  it('sends the joystick setting when it changes, and the DOM stick only while it is picked', async () => {
    const moveInput = { current: { x: 0, z: 0.5 } };
    const { frame, sent } = await setup({ options: { moveInput } });

    frame.step(0.016, 1000);
    frame.step(0.016, 1016);
    frame.options = { ...frame.options, joystickKind: 'dom' };
    frame.step(0.016, 1032);
    frame.options = { ...frame.options, joystickKind: 'bevy' };
    frame.step(0.016, 1048);

    expect(sent.joystickKinds).toEqual(['bevy', 'dom', 'bevy']);
    expect(sent.sticks).toEqual([[0, 0.5]]);
  });

  it('auto-walks at walk speed without input, and keys cancel it', async () => {
    const autoWalk = { intent: vi.fn(() => ({ x: 1, z: 0 })), cancel: vi.fn() };
    const { frame, steps, store, setIntent } = await setup({ options: { autoWalk } });

    frame.step(0.016, 1000);
    expect(steps[0]).toMatchObject({ dir: { x: 1, z: 0 }, speed: store.getState().worldSettings.playerSpeed });
    expect(autoWalk.cancel).not.toHaveBeenCalled();

    setIntent({ dir: [0, -1], run: false, jump: false, yaw: 0, deflection: 0 });
    frame.step(0.016, 1016);
    expect(autoWalk.cancel).toHaveBeenCalledOnce();
    expect(steps[1]!.dir).toEqual({ x: 0, z: -1 });
  });

  it("falls back to the DOM joystick on a view that can't show the Bevy one", async () => {
    const moveInput = { current: { x: 0.5, z: 0 } };
    const { frame, sent, domJoystick } = await setup({ options: { moveInput }, bevyStick: false });

    frame.step(0.016, 1000);

    expect(domJoystick).toEqual([true]);
    expect(sent.sticks).toEqual([[0.5, 0]]);
  });

  it('neither the stick nor auto-walk moves the player in free-fly', async () => {
    const autoWalk = { intent: vi.fn(() => ({ x: 1, z: 0 })), cancel: vi.fn() };
    const { frame, steps } = await setup({
      intent: { dir: [0, -1], run: false, jump: false, yaw: 0, deflection: 1 },
      options: { cameraMode: 'free-fly', autoWalk },
    });

    frame.step(0.016, 1000);

    expect(steps[0]!.dir).toEqual({ x: 0, z: 0 });
  });

  it('turns the view\'s input off while a text field has focus, and back on after', async () => {
    const { frame, inputEnabled } = await setup();
    const input = document.body.appendChild(document.createElement('input'));

    frame.step(0.016, 1000);
    input.focus();
    frame.step(0.016, 1016);
    input.blur();
    frame.step(0.016, 1032);

    expect(inputEnabled).toEqual([true, false, true]);
    input.remove();
  });
});

const WALK_EAST: MoveIntent = { dir: [1, 0], run: false, jump: false, yaw: 0, deflection: 0 };
const POOL = { id: 'pool', minX: -50, maxX: 50, minZ: -50, maxZ: 50, baseY: -5, surfaceY: 50 };

describe('RoomFrame gameplay', () => {
  it('swims in water: the pose is broadcast, the swim speed replaces the walk and Space does nothing', async () => {
    const { frame, steps, store } = await setup({
      intent: { ...WALK_EAST, run: true, jump: true },
      options: { motionWorld: emptyWorld({ waterAt: () => POOL }) },
    });

    frame.step(0.016, 1000);

    expect(store.getState().players.alice!.pose).toBe('swim');
    expect(steps[0]).toMatchObject({ dir: { x: 1, z: 0 }, speed: SWIM_STRATEGIES.stroke.speed, jump: false });
  });

  it('F mounts the ride in reach, which then accelerates from rest; F again gets off', async () => {
    const rideNear = vi.fn(() => ({ vehicle: 'scooter' as const, rackId: 'rack-1' }));
    const { frame, steps, store, hold } = await setup({ intent: WALK_EAST, options: { motionWorld: emptyWorld({ rideNear }) } });

    hold('f');
    frame.step(0.1, 1000);
    expect(store.getState().players.alice).toMatchObject({ pose: 'scooter', poseRack: 'rack-1' });
    expect(steps[0]!.speed).toBeCloseTo(RIDE_STRATEGIES.scooter.accel * 0.1);

    frame.step(0.1, 1100);
    expect(steps[1]!.speed).toBeCloseTo(RIDE_STRATEGIES.scooter.accel * 0.2);

    hold();
    frame.step(0.1, 1200);
    hold('f');
    frame.step(0.1, 1300);
    expect(store.getState().players.alice!.pose ?? null).toBeNull();
  });

  it('E sits, and E again while seated stands up', async () => {
    const seating = { sit: vi.fn(), stand: vi.fn(), setStandingCheck: vi.fn() };
    const { frame, actions, hold } = await setup({ options: { seating } });

    hold('e');
    frame.step(0.016, 1000);
    frame.step(0.016, 1016);
    expect(seating.sit).toHaveBeenCalledOnce();

    actions.upsertPlayer({ id: 'alice', seat: 'chair-1' });
    hold();
    frame.step(0.016, 1032);
    hold('e');
    frame.step(0.016, 1048);
    expect(seating.stand).toHaveBeenCalledOnce();
    expect(seating.sit).toHaveBeenCalledOnce();
  });

  it('runs the touch buttons as their keys: Jump jumps once per tap, Sprint runs', async () => {
    const actionInput = new ActionInput();
    const { frame, steps, store } = await setup({ intent: WALK_EAST, options: { actionInput } });

    actionInput.press('jump');
    actionInput.press('sprint');
    frame.step(0.016, 1000);
    frame.step(0.016, 1016);

    const { playerSpeed, runSpeedMultiplier } = store.getState().worldSettings;
    expect(steps.map((s) => s.jump)).toEqual([true, false]);
    expect(steps[0]).toMatchObject({ running: true, speed: playerSpeed * runSpeedMultiplier });
  });

  it('Q pokes the player in front once per cooldown', async () => {
    const { frame, actions, store, sentNet, bus, hold } = await setup();
    const { pos } = store.getState().players.alice!;
    // Yaw 0 faces -Z.
    actions.upsertPlayer({ id: 'bob', pos: { x: pos.x, y: pos.y, z: pos.z - 1 } });
    const received: (string | null)[] = [];
    bus.on('poke:received', ({ targetId }) => received.push(targetId));

    hold('q');
    frame.step(0.016, 1000);
    hold();
    frame.step(0.016, 1100);
    hold('q');
    frame.step(0.016, 1200);
    hold();
    frame.step(0.016, 2000);
    hold('q');
    frame.step(0.016, 2100);

    expect(sentNet.pokes).toEqual(['bob', 'bob']);
    expect(received).toEqual(['bob', 'bob']);
  });

  it('F takes the wand from a stand, the digits pick a spell and Q casts it as far as the wall', async () => {
    const { frame, store, sentNet, hold } = await setup({
      options: { motionWorld: emptyWorld({ wandStandNear: () => 'stand-1' }) },
    });

    hold('f');
    frame.step(0.016, 1000);
    expect(store.getState().players.alice).toMatchObject({ held: 'wand', heldStand: 'stand-1' });

    hold('2');
    frame.step(0.016, 1016);
    expect(store.getState().selectedSpell).toBe(SPELL_IDS[1]);

    hold('q');
    frame.step(0.016, 1032);
    expect(sentNet.pokes).toEqual([]);
    expect(sentNet.casts).toEqual([expect.objectContaining({ spell: SPELL_IDS[1], distance: 4 })]);
  });

  it('a click on the canvas casts in first and third person only', async () => {
    const { frame, sentNet, click, actions } = await setup({ options: { cameraMode: 'fixed' } });
    actions.setSelfHeld('wand', 'stand-1');

    click();
    frame.step(0.016, 1000);
    expect(sentNet.casts).toHaveLength(0);

    frame.options = { ...frame.options, cameraMode: 'third-person' };
    click();
    frame.step(0.016, 3000);
    expect(sentNet.casts).toHaveLength(1);
  });

  it('offers what is in reach to the prompt, unanchored, a few times a second, and nothing while auto-walking', async () => {
    const interactions = new InteractionStore();
    const seating = { sit: vi.fn(), stand: vi.fn(), setStandingCheck: vi.fn(), getState: () => ({ nearbySeatId: 'chair-1' }) };
    const autoWalk = { intent: vi.fn((): { x: number; z: number } | null => null), cancel: vi.fn() };
    const { frame } = await setup({
      options: { interactions, seating, autoWalk, motionWorld: emptyWorld({ anchorOf: () => ({ x: 1, y: 1, z: 1 }) }) },
    });

    frame.step(0.016, 1000);
    expect(interactions.get()).toEqual({ options: [expect.objectContaining({ kind: 'sit', key: 'E' })], anchor: null });

    autoWalk.intent.mockReturnValue({ x: 1, z: 0 });
    frame.step(0.016, 1100);
    expect(interactions.get().options).toHaveLength(1);
    frame.step(0.016, 1200);
    expect(interactions.get()).toBe(NO_PROMPT);
  });

  it('a spell that holds the player still stops it walking and jumping', async () => {
    const { frame, steps, store } = await setup({ intent: { ...WALK_EAST, jump: true } });
    store.setState(() => ({ spellEffects: { alice: { spell: 'levitate', casterId: 'bob', startsAt: 0, endsAt: 1e9 } } }));

    frame.step(0.016, 1000);

    expect(steps[0]).toMatchObject({ speed: 0, jump: false });
  });
});
