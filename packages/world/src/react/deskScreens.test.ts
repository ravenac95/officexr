import { afterEach, describe, expect, it } from 'vitest';
import type { OfficeState } from '@officexr/sdk';
import { SCREEN_OFF_DELAY_SEC, SCREEN_POWER_UP_SEC, type DeskScreenLink } from '../app/desk-screens.ts';
import { DeskScreens } from './deskScreens.ts';

const link: DeskScreenLink = {
  deskAreaId: 'desk-a',
  seatId: 'chair',
  monitorInstanceId: 'monitor',
  monitor: { x: 0, z: 0 },
  chair: { x: 0, z: 2 },
};
const seated = { players: { me: { seat: 'chair' } } } as unknown as Pick<OfficeState, 'players'>;
const empty = { players: { me: { seat: null } } } as unknown as Pick<OfficeState, 'players'>;
type MarkerWindow = { __OFFICE_DESK_SCREENS__?: { screens: { on: boolean; glow: number }[] } };

afterEach(() => {
  delete (globalThis as MarkerWindow).__OFFICE_DESK_SCREENS__;
});

describe('DeskScreens', () => {
  it("powers a monitor up while its chair is taken and off a second after it's left", () => {
    const screens = new DeskScreens('test');
    screens.setLinks([link]);
    expect(screens.step(empty, 0.1)).toEqual([]);
    expect(screens.step(empty, 0.1)).toBeNull();

    let lit = null;
    for (let t = 0; t <= SCREEN_POWER_UP_SEC + 0.05; t += 0.05) {
      lit = screens.step(seated, 0.05) ?? lit;
    }
    expect(lit).toEqual([{ id: 'monitor', glow: 1, toward: [0, 2] }]);
    expect(screens.step(seated, 0.05)).toBeNull();
    expect((globalThis as MarkerWindow).__OFFICE_DESK_SCREENS__?.screens[0]).toMatchObject({ on: true, glow: 1 });

    expect(screens.step(empty, SCREEN_OFF_DELAY_SEC / 2)).toBeNull();
    expect(screens.step(empty, SCREEN_OFF_DELAY_SEC)).toEqual([]);
    screens.dispose();
    expect((globalThis as MarkerWindow).__OFFICE_DESK_SCREENS__).toBeUndefined();
  });

  it('keeps a lit monitor lit across a re-link and puts out one no longer linked', () => {
    const screens = new DeskScreens('test');
    screens.setLinks([link]);
    for (let i = 0; i < 20; i++) {
      screens.step(seated, 0.05);
    }
    screens.setLinks([{ ...link }]);
    expect(screens.step(seated, 0.05)).toEqual([{ id: 'monitor', glow: 1, toward: [0, 2] }]);
    screens.setLinks([]);
    expect(screens.step(seated, 0.05)).toEqual([]);
    screens.dispose();
  });
});
