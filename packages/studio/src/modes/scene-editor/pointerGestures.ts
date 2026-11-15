type Point = readonly [number, number];

/** A press whose pointer strays further than this (CSS px) is a drag, not a click. */
export const CLICK_SLOP_PX = 4;

/** One press: where it began and how far the pointer has strayed since. */
class Press {
  private start: Point | null = null;
  private strayed = 0;

  begin(at: Point) {
    this.start = at;
    this.strayed = 0;
  }

  move(at: Point) {
    if (this.start) {
      this.strayed = Math.max(this.strayed, Math.abs(at[0] - this.start[0]), Math.abs(at[1] - this.start[1]));
    }
  }

  /** Ends the press: true when it was a click. */
  end(at: Point): boolean {
    this.move(at);
    const click = this.start !== null && this.strayed <= CLICK_SLOP_PX;
    this.start = null;
    return click;
  }
}

/**
 * The scene editor's press bookkeeping across its two inputs: Bevy's picks, which arrive a frame
 * late and only over a target, and the DOM's pointer events on the canvas, which are immediate.
 */
export class PointerGestures {
  private readonly pick = new Press();
  private readonly dom = new Press();
  private held = false;
  private domCommitted = false;

  /** Whether the left button is down now, as the DOM saw it. */
  get leftHeld(): boolean {
    return this.held;
  }

  domDown(button: number, at: Point) {
    this.domCommitted = false;
    if (button !== 0) return;
    this.held = true;
    this.dom.begin(at);
  }

  domMove(at: Point) {
    this.dom.move(at);
  }

  /** A left release on the canvas: true when the press was a click. */
  domUp(button: number, at: Point): boolean {
    return button === 0 && this.dom.end(at);
  }

  /** The DOM handled this press's click itself; Bevy's `click` for it, a frame later, is ignored. */
  domCommit() {
    this.domCommitted = true;
  }

  /** A left release or cancel anywhere on the page. */
  released() {
    this.held = false;
  }

  pickDown(at: Point) {
    this.pick.begin(at);
  }

  pickDrag(at: Point) {
    this.pick.move(at);
  }

  /** Whether Bevy's `click` at `at` should act. */
  pickClick(at: Point): boolean {
    return this.pick.end(at) && !this.domCommitted;
  }
}
