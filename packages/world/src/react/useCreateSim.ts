import { useEffect, useEffectEvent, useState } from 'react';
import RAPIER from '@dimforge/rapier3d-compat';
import type { SimCore } from '../sim/SimCore.ts';
import { TsSimCore } from '../sim/TsSimCore.ts';
import { loadWasmSimCore } from '../sim/WasmSimCore.ts';

export type CreateSim = (charRadius: number) => SimCore;

/** Which `SimCore` runs the local player; `'rust'` is the wasm build of `crates/officexr-sim`. */
export type SimImplementation = 'ts' | 'rust';

const createTsSimCore: CreateSim = (charRadius) => new TsSimCore(charRadius);

/** The factory for `sim`, or null while its Rapier build loads. */
export function useCreateSim(sim: SimImplementation, onError?: (error: Error) => void): CreateSim | null {
  const [loaded, setLoaded] = useState<{ sim: SimImplementation; create: CreateSim } | null>(null);
  const reportError = useEffectEvent((error: Error) => onError?.(error));
  useEffect(() => {
    let live = true;
    const load = sim === 'rust' ? loadWasmSimCore() : RAPIER.init().then(() => createTsSimCore);
    load.then(
      (create) => {
        if (live) {
          setLoaded({ sim, create });
        }
      },
      (error: Error) => reportError(error),
    );
    return () => {
      live = false;
    };
  }, [sim]);
  return loaded?.sim === sim ? loaded.create : null;
}
