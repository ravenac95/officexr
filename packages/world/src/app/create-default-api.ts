/**
 * Wires the default `ApplicationApi` for production: catalog backed by
 * the bundled default JSON + a live fetch of `/api/world-object-kinds`,
 * geometry at the project-wide `VOXEL_SIZE`, and so on.
 *
 * Tests construct an `ApplicationApi` directly by composing service
 * implementations (real or mocked), without calling this function.
 */

import defaultCatalogJson from '../../world-object-kinds.default.json' with { type: 'json' };
import { validateWorldObjectKindCatalog } from '../scenes/world-object-kinds-schema.ts';
import { VOXEL_SIZE } from '../renderer/config.ts';
import { createCatalogService } from './catalog-service.ts';
import { createInstanceGeometry } from './geometry-service.ts';
import { createRoomService } from './room-service.ts';
import { createSceneService } from './scene-service.ts';
import type { ApplicationApi } from './types.ts';

export interface CreateDefaultApiOptions {
  voxelSize?: number;
  apiPath?: string;
  fetcher?: (url: string) => Promise<Response>;
}

export function createDefaultApi(
  opts: CreateDefaultApiOptions = {},
): ApplicationApi {
  const voxelSize = opts.voxelSize ?? VOXEL_SIZE;

  const catalog = createCatalogService({
    defaultCatalog: validateWorldObjectKindCatalog(defaultCatalogJson),
    apiPath: opts.apiPath,
    fetcher: opts.fetcher,
  });
  const geometry = createInstanceGeometry({ catalog, voxelSize });
  const rooms = createRoomService({ geometry });
  const scenes = createSceneService();

  return { voxelSize, catalog, geometry, rooms, scenes };
}
