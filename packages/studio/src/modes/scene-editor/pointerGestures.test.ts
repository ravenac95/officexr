import { describe, expect, it } from 'vitest';
import { PointerGestures } from './pointerGestures.ts';

describe('PointerGestures', () => {
  it('a press that jitters 2 px is still a click, on both inputs', () => {
    const g = new PointerGestures();
    g.domDown(0, [100, 100]);
    g.pickDown([10, 10]);
    g.domMove([102, 99]);
    g.pickDrag([12, 9]);
    expect(g.domUp(0, [101, 101])).toBe(true);
    expect(g.pickClick([11, 11])).toBe(true);
  });

  it('a press that strays 5 px is a drag, even if it comes back', () => {
    const g = new PointerGestures();
    g.domDown(0, [100, 100]);
    g.pickDown([10, 10]);
    g.domMove([105, 100]);
    g.pickDrag([10, 15]);
    expect(g.domUp(0, [100, 100])).toBe(false);
    expect(g.pickClick([10, 10])).toBe(false);
  });

  it('holds the left button from the DOM press to any release, so a late Bevy down starts no drag', () => {
    const g = new PointerGestures();
    g.domDown(0, [0, 0]);
    expect(g.leftHeld).toBe(true);
    g.domUp(0, [0, 0]);
    g.released();
    // Bevy's `down` for that press arrives now, a frame late.
    expect(g.leftHeld).toBe(false);
    g.domDown(2, [0, 0]);
    expect(g.leftHeld).toBe(false);
  });

  it("ignores Bevy's late click for a press the DOM committed, until the next press", () => {
    const g = new PointerGestures();
    g.domDown(0, [0, 0]);
    g.pickDown([0, 0]);
    expect(g.domUp(0, [0, 0])).toBe(true);
    g.domCommit();
    expect(g.pickClick([0, 0])).toBe(false);

    g.domDown(2, [0, 0]);
    g.pickDown([0, 0]);
    expect(g.pickClick([0, 0])).toBe(true);
  });
});
