import {
  currentVisual,
  SPELL_IDS,
  spellForDigit,
  type Actions,
  type OfficeState,
  type PlayerState,
  type SyncEngine,
  type Vec3,
} from '@officexr/sdk';
import { interactionPrompt, NO_PROMPT, type InteractionPrompt, type InteractionStore } from '../input/interaction.ts';
import { buildCast } from '../motion/cast.ts';
import { POKE_COOLDOWN_S, pokeFacing } from '../motion/poke.ts';
import { newItemsState, stepSelfItems, type ItemsAction } from '../motion/self-items.ts';
import { newSelfMotion, stepSelfMotion, type MotionWorld, type SelfMotionOutput } from '../motion/self-motion.ts';
import { feetPosition } from '../physics/rules.ts';
import { assertNever } from '../renderer/assertNever.ts';
import type { SeatingSource } from '../renderer/seated.ts';
import type { SelfWorld } from '../renderer/useMotionWorld.ts';

/** How often the interaction prompt is refreshed. */
export const PROMPT_INTERVAL_MS = 150;

const SPELL_DIGITS = SPELL_IDS.map((_, i) => String(i + 1));

export interface SelfPlayDeps {
  actions: Pick<Actions, 'setSelfPose' | 'setSelfHeld' | 'setSelectedSpell' | 'receiveSpellCast' | 'receivePoke'>;
  sync: Pick<SyncEngine, 'sendCast' | 'sendPoke'>;
  selfId: string;
}

/** One frame's view of the local player, for `act`. */
export interface SelfActFrame {
  state: OfficeState;
  self: PlayerState;
  nowMs: number;
  /** A left click on the canvas casts, as Q does. */
  castClicked: boolean;
  world: SelfWorld;
  seating?: SeatingSource;
  interactions?: InteractionStore;
  /** Auto-walk is steering: no prompt shows. */
  autoWalking: boolean;
  /** How far a spell flies from `origin` along `dir` before a wall stops it, at most `maxDistance`. */
  wallDistance(origin: Vec3, dir: Vec3, maxDistance: number): number;
}

export interface SelfActOutcome {
  /** F went down for a ride this frame, not for the wand. */
  mount: boolean;
  /** Space went down this frame (the touch Jump button; Bevy reports the key's own press). */
  jumpPressed: boolean;
  /** The player's spell effect scales its speed; 0 also stops it jumping. */
  moveScale: number;
}

/** One frame of the local player's movement, for `move`. */
export interface SelfMoveFrame {
  self: PlayerState;
  feet: Vec3;
  /** World-space unit direction the player steers in, or null for none. */
  dir: { x: number; z: number } | null;
  sprint: boolean;
  mount: boolean;
  dtSec: number;
  world: MotionWorld;
}

/**
 * The local player's gameplay beside walking, as upstream's `SceneFrame` ran
 * it: E sits and stands, F takes or returns the wand and mounts or leaves a
 * ride, Q pokes or casts, the digits pick a spell, the interaction prompt
 * follows what is in reach, and swimming and riding replace the walk.
 * Reads keys as `ActionInput.mergeInto` gives them, so the touch buttons run
 * the same code as their keys.
 */
export class SelfPlay {
  private readonly motion = newSelfMotion();
  private readonly items = newItemsState();
  private wasDown: ReadonlySet<string> = new Set();
  private pressed: ReadonlySet<string> = new Set();
  private lastPokeAt = -Infinity;
  private lastPromptAt = -Infinity;

  constructor(private readonly deps: SelfPlayDeps) {}

  /** Takes this frame's held keys; call once at the top of every frame, before the other calls. */
  begin(keys: ReadonlySet<string>): void {
    this.pressed = new Set([...keys].filter((k) => !this.wasDown.has(k)));
    this.wasDown = new Set(keys);
  }

  /** E sits on the seat in reach, or stands up when `seated`. */
  sit(seating: SeatingSource | undefined, seated: boolean): void {
    if (!this.pressed.has('e')) {
      return;
    }
    if (seated) {
      seating?.stand();
    } else {
      seating?.sit();
    }
  }

  /** The wand, spells, pokes and the interaction prompt. */
  act(f: SelfActFrame): SelfActOutcome {
    const { state, self, nowMs } = f;
    const { actions, sync, selfId } = this.deps;
    const feet = feetPosition(self.pos, state.worldSettings.charRadius);
    const fPressed = this.pressed.has('f');
    const qPressed = this.pressed.has('q');
    const digit = SPELL_DIGITS.find((d) => this.pressed.has(d));
    const holding = self.held === 'wand';
    this.items.spell = state.selectedSpell;
    const items = stepSelfItems(this.items, {
      now: nowMs,
      fPressed,
      castPressed: qPressed || f.castClicked,
      digitSpell: digit ? spellForDigit(digit) : null,
      holding,
      posed: (self.pose ?? null) !== null,
      seated: self.seat != null,
      standNear: fPressed && !holding ? f.world.wandStandNear(feet) : null,
    });
    if (this.items.spell !== state.selectedSpell) {
      actions.setSelectedSpell(this.items.spell);
    }
    if (items.action) {
      this.runItemAction(items.action, f, feet);
    }
    if (qPressed && !items.suppressPoke && nowMs - this.lastPokeAt > POKE_COOLDOWN_S * 1000) {
      this.lastPokeAt = nowMs;
      pokeFacing(state, selfId, actions, sync, nowMs);
    }
    this.offerPrompt(f, feet);
    return {
      mount: fPressed && !items.suppressMount,
      jumpPressed: this.pressed.has(' '),
      moveScale: currentVisual(state.spellEffects, selfId, nowMs)?.moveSpeedScale ?? 1,
    };
  }

  /** A seat ends swimming and riding. */
  seat(): void {
    if (this.motion.pose !== null) {
      this.motion.pose = null;
      this.deps.actions.setSelfPose(null);
    }
  }

  /**
   * Water and rides: switches the pose (and broadcasts it), and returns the
   * direction and speed to step with in place of the walk's (`speed` null
   * keeps the walk), and whether a jump may start.
   */
  move(m: SelfMoveFrame): SelfMotionOutput {
    const out = stepSelfMotion(
      this.motion,
      {
        feet: m.feet,
        dir: m.dir,
        sprint: m.sprint,
        mount: m.mount,
        heading: Math.atan2(-Math.sin(m.self.yaw), -Math.cos(m.self.yaw)),
        dt: m.dtSec,
      },
      m.world,
    );
    if ((m.self.pose ?? null) !== this.motion.pose || (m.self.poseRack ?? null) !== this.motion.rack) {
      this.deps.actions.setSelfPose(this.motion.pose, this.motion.rack);
    }
    return out;
  }

  private runItemAction(action: NonNullable<ItemsAction>, f: SelfActFrame, feet: Vec3): void {
    const { actions, sync, selfId } = this.deps;
    switch (action.kind) {
      case 'pickup':
        actions.setSelfHeld('wand', action.standId);
        break;
      case 'putback':
        actions.setSelfHeld(null);
        break;
      case 'cast': {
        const charRadius = f.state.worldSettings.charRadius;
        const others = Object.values(f.state.players)
          .filter((p) => p.id !== selfId && !p.isDead)
          .map((p) => ({ id: p.id, feet: feetPosition(p.pos, charRadius) }));
        const cast = buildCast(action.spell, { feet, yaw: f.self.yaw }, others, f.wallDistance);
        actions.receiveSpellCast(selfId, cast, f.nowMs);
        sync.sendCast(cast);
        break;
      }
      default:
        assertNever(action);
    }
  }

  private offerPrompt(f: SelfActFrame, feet: Vec3): void {
    if (!f.interactions || f.nowMs - this.lastPromptAt <= PROMPT_INTERVAL_MS) {
      return;
    }
    this.lastPromptAt = f.nowMs;
    const nearbySeatId = f.seating?.getState?.().nearbySeatId ?? null;
    f.interactions.offer(
      f.autoWalking
        ? NO_PROMPT
        : unanchored(
            interactionPrompt(
              { pose: this.motion.pose, seatedOn: f.self.seat ?? null, nearbySeatId, feet, holdingWand: f.self.held === 'wand' },
              f.world,
            ),
          ),
    );
  }
}

// ponytail: the Bevy view draws no pill above the object yet, so every prompt goes to the HUD's
// unanchored panel; keep the anchor once the view can place a label over it.
function unanchored(prompt: InteractionPrompt): InteractionPrompt {
  return prompt.anchor === null ? prompt : { ...prompt, anchor: null };
}
