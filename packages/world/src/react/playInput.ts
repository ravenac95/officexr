import { LOOK_DRAG_THRESHOLD_PX } from '../renderer/desktopLook.ts';
import { hasCommandModifier, isTypingTarget } from './useWorldFocus.ts';

const LEFT = 0;

/**
 * The keys and canvas clicks the play view's gameplay reads that Bevy's
 * controls don't: E, F, Q and the spell digits by `KeyboardEvent.key`
 * (lowercased, like `ActionInput`'s keys), and a left click on the canvas
 * that is not the end of a mouse-look drag.
 */
export class PlayInput {
  private readonly down = new Set<string>();
  private enabled = true;
  /** CSS px the mouse moved since a left press on the surface; null with no press. */
  private travel: { x: number; y: number } | null = null;
  private clicked = false;

  constructor(
    private readonly keys: EventTarget,
    /** Where canvas clicks bubble to: the view's container. */
    private readonly surface: EventTarget,
  ) {
    keys.addEventListener('keydown', this.onKeyDown);
    keys.addEventListener('keyup', this.onKeyUp);
    keys.addEventListener('blur', this.clear);
    keys.addEventListener('mousemove', this.onMouseMove);
    surface.addEventListener('mousedown', this.onMouseDown);
    surface.addEventListener('click', this.onClick);
  }

  /** Off while the world is unfocused: held keys let go and nothing new is heard. */
  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (!enabled) {
      this.clear();
    }
  }

  held(): ReadonlySet<string> {
    return this.down;
  }

  /** Whether the canvas was clicked since the last call. */
  takeClick(): boolean {
    const clicked = this.clicked;
    this.clicked = false;
    return clicked;
  }

  dispose(): void {
    this.keys.removeEventListener('keydown', this.onKeyDown);
    this.keys.removeEventListener('keyup', this.onKeyUp);
    this.keys.removeEventListener('blur', this.clear);
    this.keys.removeEventListener('mousemove', this.onMouseMove);
    this.surface.removeEventListener('mousedown', this.onMouseDown);
    this.surface.removeEventListener('click', this.onClick);
  }

  private readonly clear = () => {
    this.down.clear();
    this.clicked = false;
  };

  private readonly onKeyDown = (e: Event) => {
    const key = e as KeyboardEvent;
    if (!this.enabled || isTypingTarget(key.target) || hasCommandModifier(key)) {
      return;
    }
    this.down.add(key.key.toLowerCase());
  };

  private readonly onKeyUp = (e: Event) => {
    this.down.delete((e as KeyboardEvent).key.toLowerCase());
  };

  private readonly onMouseDown = (e: Event) => {
    const mouse = e as MouseEvent;
    if (mouse.button === LEFT) {
      this.travel = { x: 0, y: 0 };
    }
  };

  // Movement, not the pointer's position: a drag that locked the pointer leaves the position still.
  private readonly onMouseMove = (e: Event) => {
    const mouse = e as MouseEvent;
    if (this.travel) {
      this.travel = { x: this.travel.x + (mouse.movementX || 0), y: this.travel.y + (mouse.movementY || 0) };
    }
  };

  // The controls turn the view on a left drag of the same threshold, so a click that ended one isn't a click.
  private readonly onClick = (e: Event) => {
    const mouse = e as MouseEvent;
    const travel = this.travel;
    this.travel = null;
    if (
      this.enabled &&
      mouse.button === LEFT &&
      mouse.target instanceof HTMLCanvasElement &&
      travel !== null &&
      Math.hypot(travel.x, travel.y) < LOOK_DRAG_THRESHOLD_PX
    ) {
      this.clicked = true;
    }
  };
}
