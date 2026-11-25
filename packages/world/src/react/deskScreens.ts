import type { OfficeState } from '@officexr/sdk';
import type { ScreenInput } from '@officexr/render-wasm';
import {
  isChairOccupied,
  isScreenSettled,
  screenGlow,
  SCREEN_OFF,
  stepScreenPower,
  type DeskScreenLink,
  type ScreenPower,
} from '../app/desk-screens.ts';
import { publishDeskScreens } from '../renderer/deskScreenMarker.ts';

interface Driven {
  link: DeskScreenLink;
  power: ScreenPower;
}

/**
 * Steps each linked desk monitor's power as its chair fills and empties (`useDeskScreenGlow.ts`),
 * and says what the view should light. Publishes the screens as `__OFFICE_DESK_SCREENS__`.
 */
export class DeskScreens {
  private driven: Driven[] = [];
  private links: readonly DeskScreenLink[] | null = null;
  /** The links changed since the view last heard which screens are lit. */
  private relinked = false;

  constructor(private readonly markerKey: string) {}

  /** Keeps the power of the monitors still linked, so a re-link doesn't flicker them. */
  setLinks(links: readonly DeskScreenLink[]): void {
    if (links === this.links) {
      return;
    }
    this.links = links;
    this.relinked = true;
    const before = new Map(this.driven.map((d) => [d.link.monitorInstanceId, d.power]));
    this.driven = links.map((link) => ({ link, power: before.get(link.monitorInstanceId) ?? SCREEN_OFF }));
    publishDeskScreens(
      this.markerKey,
      this.driven.map((d) => ({
        deskAreaId: d.link.deskAreaId,
        monitorInstanceId: d.link.monitorInstanceId,
        get on() {
          return d.power.on;
        },
        get glow() {
          return screenGlow(d.power);
        },
      })),
    );
  }

  /** The lit screens after `dtSec`, or `null` when none changed. */
  step(state: Pick<OfficeState, 'players'>, dtSec: number): ScreenInput[] | null {
    let changed = this.relinked;
    this.relinked = false;
    for (const d of this.driven) {
      const occupied = isChairOccupied(state, d.link.seatId);
      if (isScreenSettled(d.power, occupied)) {
        continue;
      }
      const before = screenGlow(d.power);
      d.power = stepScreenPower(d.power, occupied, dtSec);
      changed ||= screenGlow(d.power) !== before;
    }
    return changed ? this.lit() : null;
  }

  lit(): ScreenInput[] {
    return this.driven.flatMap(({ link, power }) => {
      const glow = screenGlow(power);
      return glow > 0
        ? [{ id: link.monitorInstanceId, glow, toward: [link.chair.x - link.monitor.x, link.chair.z - link.monitor.z] }]
        : [];
    });
  }

  dispose(): void {
    publishDeskScreens(this.markerKey, null);
  }
}
