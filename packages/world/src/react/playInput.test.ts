// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { LOOK_DRAG_THRESHOLD_PX } from '../renderer/desktopLook.ts';
import { PlayInput } from './playInput.ts';

function setup() {
  const surface = document.body.appendChild(document.createElement('div'));
  const canvas = surface.appendChild(document.createElement('canvas'));
  const input = new PlayInput(window, surface);
  const key = (type: 'keydown' | 'keyup', key: string, init: KeyboardEventInit = {}, target: EventTarget = window) =>
    target.dispatchEvent(new KeyboardEvent(type, { key, bubbles: true, ...init }));
  const move = (movementX: number) => {
    const e = new MouseEvent('mousemove', { bubbles: true });
    Object.defineProperty(e, 'movementX', { value: movementX });
    window.dispatchEvent(e);
  };
  const press = (on: Element = canvas) => on.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
  const click = (on: Element = canvas) => on.dispatchEvent(new MouseEvent('click', { button: 0, bubbles: true }));
  return { surface, canvas, input, key, move, press, click };
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('PlayInput', () => {
  it('holds keys by their lowercased key until released', () => {
    const { input, key } = setup();
    key('keydown', 'F');
    key('keydown', 'q');
    key('keyup', 'f');
    expect([...input.held()]).toEqual(['q']);
    input.dispose();
  });

  it('ignores keys typed into a field or held with a command modifier', () => {
    const { input, key } = setup();
    const field = document.body.appendChild(document.createElement('input'));
    key('keydown', 'e', {}, field);
    key('keydown', 'q', { ctrlKey: true });
    expect(input.held().size).toBe(0);
    input.dispose();
  });

  it('lets go of every key when disabled or the window blurs, and hears none while disabled', () => {
    const { input, key } = setup();
    key('keydown', 'e');
    input.setEnabled(false);
    expect(input.held().size).toBe(0);
    key('keydown', 'e');
    expect(input.held().size).toBe(0);
    input.setEnabled(true);
    key('keydown', 'f');
    window.dispatchEvent(new Event('blur'));
    expect(input.held().size).toBe(0);
    input.dispose();
  });

  it('takes a canvas click once, but not one that ended a mouse-look drag or landed off the canvas', () => {
    const { input, press, click, move, surface } = setup();
    press();
    move(LOOK_DRAG_THRESHOLD_PX - 1);
    click();
    expect(input.takeClick()).toBe(true);
    expect(input.takeClick()).toBe(false);

    press();
    move(LOOK_DRAG_THRESHOLD_PX);
    click();
    expect(input.takeClick()).toBe(false);

    press(surface);
    click(surface);
    expect(input.takeClick()).toBe(false);
    input.dispose();
  });
});
