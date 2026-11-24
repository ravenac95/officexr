import { memo, useCallback, useEffect, useImperativeHandle, useRef, useState, type Ref } from 'react';
import type { PlayerAnchor } from '@officexr/render-wasm';
import type { Bus, OfficeState, Store } from '@officexr/sdk';
import type { FrozenAvatar } from '../app/frozen-avatars.ts';
import { frozenStatusLabel } from '../app/frozen-status.ts';
import { PlayerBubbles } from '../renderer/PlayerBubbles.tsx';
import { PlayerNameTag } from '../renderer/PlayerNameTag.tsx';
import '../renderer/FrozenAvatars.css';
import '../renderer/PlayerNameTag.css';
import { frozenAvatarId } from './roomFrame.ts';
import { useStoreSelector } from './useStoreSelector.ts';

/** Room for the ping bell Bevy hangs just over a hovered head, under the stack. */
export const BELL_SPACE_PX = 32;

/** What the frame calls on the overhead layer. */
export interface PlayerOverheadsHandle {
  /** Moves each stack to its player's anchor; a stack without one hides. */
  place(anchors: readonly PlayerAnchor[]): void;
  /** Opens or closes an away claimant's card. */
  toggleFrozen(login: string): void;
}

const selectIds = (s: OfficeState): string => Object.keys(s.players).sort().join('|');

/**
 * The DOM drawn over each avatar's head (`PlayerOverhead.tsx` and `FrozenAvatars.tsx`'s tags):
 * emote and raised-hand bubbles and the name and status tag over each player, and the "z z" tag
 * and card over each away claimant. Bevy reports where each head is (`playerAnchors`); `place`
 * moves the stacks there each frame by writing their transforms, so moving re-renders nothing.
 */
// DOM over the canvas rather than a Bevy primitive: the tags need the browser's emoji and text
// fonts and the card has buttons, the "DOM UI sits beside the canvas" half of CLAUDE.md's rule.
export const PlayerOverheads = memo(function PlayerOverheads(props: {
  store: Store;
  bus: Bus;
  selfId: string;
  frozen: readonly FrozenAvatar[];
  onPingFrozen?: (login: string) => void;
  ref?: Ref<PlayerOverheadsHandle>;
}) {
  const { store, bus, selfId, frozen, onPingFrozen } = props;
  const ids = useStoreSelector(store, selectIds);
  const stacks = useRef(new Map<string, HTMLDivElement>());
  const [openLogin, setOpenLogin] = useState<string | null>(null);
  useEffect(() => {
    if (openLogin !== null && !frozen.some((f) => f.login === openLogin)) {
      setOpenLogin(null);
    }
  }, [frozen, openLogin]);

  useImperativeHandle(
    props.ref,
    () => ({
      place(anchors) {
        placeStacks(stacks.current, anchors);
      },
      toggleFrozen(login) {
        setOpenLogin((cur) => (cur === login ? null : login));
      },
    }),
    [],
  );

  const stackRef = useCallback(
    (id: string) => (el: HTMLDivElement | null) => {
      if (el) {
        stacks.current.set(id, el);
      } else {
        stacks.current.delete(id);
      }
    },
    [],
  );

  return (
    <div style={{ position: 'absolute', inset: 0, overflow: 'hidden', pointerEvents: 'none' }}>
      {ids
        .split('|')
        .filter(Boolean)
        .map((id) => (
          <div key={id} ref={stackRef(id)} style={HIDDEN_STACK}>
            <div className="player-overhead">
              <PlayerBubbles store={store} bus={bus} playerId={id} paused={false} />
              <PlayerNameTag store={store} playerId={id} isSelf={id === selfId} paused={false} />
            </div>
          </div>
        ))}
      {frozen.map((f) => {
        const open = openLogin === f.login;
        const toggle = () => setOpenLogin((cur) => (cur === f.login ? null : f.login));
        const status = frozenStatusLabel(f);
        return (
          <div key={f.login} ref={stackRef(frozenAvatarId(f.login))} style={HIDDEN_STACK}>
            <div className="frozen-overhead" style={{ pointerEvents: 'auto' }}>
              <div className="frozen-tag" onClick={toggle}>
                <div className="frozen-tag-zzz" aria-hidden>
                  z z
                </div>
                <div className="frozen-tag-name">{f.name}</div>
                <div className="frozen-tag-status">{status}</div>
              </div>
              {open && (
                <div className="frozen-card" role="dialog" aria-label={`${f.name} is away`}>
                  <div className="frozen-card-name">{f.name}</div>
                  <div className="frozen-card-status">{status}</div>
                  <div className="frozen-card-actions">
                    {onPingFrozen && (
                      <button type="button" onClick={() => onPingFrozen(f.login)}>
                        Ping
                      </button>
                    )}
                    <button type="button" onClick={toggle} aria-label="Close">
                      Close
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
});

const HIDDEN_STACK = { position: 'absolute', left: 0, top: 0, visibility: 'hidden' } as const;

/** Moves each stack to its anchor, leaving room under it for a shown ping bell. */
export function placeStacks(stacks: ReadonlyMap<string, HTMLElement>, anchors: readonly PlayerAnchor[]): void {
  const byId = new Map(anchors.map((a) => [a.id, a]));
  for (const [id, el] of stacks) {
    const a = byId.get(id);
    if (!a || a.x === null || a.y === null) {
      if (el.style.visibility !== 'hidden') {
        el.style.visibility = 'hidden';
      }
      continue;
    }
    const lift = a.bell ? BELL_SPACE_PX : 0;
    const transform = `translate3d(${a.x.toFixed(1)}px, ${(a.y - lift).toFixed(1)}px, 0)`;
    if (el.style.transform !== transform) {
      el.style.transform = transform;
    }
    if (el.style.visibility !== '') {
      el.style.visibility = '';
    }
  }
}
