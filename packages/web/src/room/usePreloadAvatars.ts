import { useEffect } from 'react';
import type { OfficeState, PlayerId, Store } from '@officexr/sdk';
import { resolveCharacter, type CatalogCharacters } from '@officexr/world/characters';
import { useCatalogCharacters } from '@officexr/world/react';

type Preload = (urls: string[]) => void;

// ponytail: officexr-render fetches each avatar when the view first spawns it; there is no preload hook to warm it yet.
const preloadInWorld: Preload = () => undefined;

function avatarUrls(state: OfficeState, selfId: PlayerId, catalog: CatalogCharacters): string[] {
  const urls = new Set([resolveCharacter(selfId, state.players[selfId]?.avatar.model, catalog).url]);
  for (const player of Object.values(state.players)) {
    urls.add(resolveCharacter(player.id, player.avatar.model, catalog).url);
  }
  return [...urls];
}

/** Starts downloading the model of the self player and of everyone in the room, before the 3D view mounts their avatars. */
export function usePreloadAvatars(store: Store | null, selfId: PlayerId | null, preload: Preload = preloadInWorld): void {
  const catalog = useCatalogCharacters();
  useEffect(() => {
    if (!store || selfId === null) {
      return;
    }
    const urls = (s: OfficeState) => avatarUrls(s, selfId, catalog);
    preload(urls(store.getState()));
    return store.subscribe(
      (s) => urls(s).join('\n'),
      (joined) => preload(joined.split('\n')),
    );
  }, [store, selfId, catalog, preload]);
}
