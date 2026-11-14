import { createDefaultApi } from '@officexr/world/app';
import type { ApplicationApi } from '@officexr/world/app';

/**
 * Composes a hermetic `ApplicationApi` for tests.
 *
 * Delegates to the production `createDefaultApi` with `apiPath: ''`,
 * which disables the live `/api/world-object-kinds` fetch and uses the
 * bundled default catalog instead. This is the single DI seam the
 * roadmap leans on: editors mounted under a provider built from this
 * api never touch the network or the dev-server filesystem.
 */
export function createTestApi(): ApplicationApi {
  return createDefaultApi({
    apiPath: '',
  });
}
