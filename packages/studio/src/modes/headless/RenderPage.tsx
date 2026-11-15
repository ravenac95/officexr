import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Overlay, WorldDiff } from '@officexr/render-wasm';
import { DEFAULT_WORLD_CONSTANTS } from '@officexr/sdk';
import { DEFAULT_EDITOR_LIGHTING, type ApplicationApi } from '@officexr/world/app';
import { blobAssetPath, useApplication, usePrimaryBevyView } from '@officexr/world/react';
import {
  FilesystemLayoutStorage,
  FilesystemMapStorage,
  FilesystemRoomStorage,
} from '@officexr/world/scenes';
import { useTestStorages } from '../../test-harness/TestStorageContext.tsx';
import type { RoomPlacements } from '../map/roomPlacements.ts';
import { studioBakedLayoutUrl } from '../useBakedLayoutBlobs.ts';
import { loadRenderContent, type RenderContent, type RenderStorages } from './renderContent.ts';
import { cameraPose, frameCamera, type Bounds, type CameraSetup } from './renderFraming.ts';
import { parseRenderParams, type RenderParams } from './renderParams.ts';
import { paths, studioApiOptions, studioFetch } from '../../apiOrigin.ts';

interface RenderSentinels {
  __officexrRenderReady?: { bounds: Bounds; metresPerPixel?: number };
  __officexrRenderError?: string;
}

const sentinels = () => window as unknown as RenderSentinels;
const BACKDROP = '#1b1d22';
/** `EndlessGrid fadeDistance={400}`. */
const GRID: Overlay = { tag: 'grid', kind: 'grid', y: 0, cellSize: 1, fade: 400 };
const READY_POLL_MS = 50;
const errorMessage = (err: unknown) => (err instanceof Error ? err.message : String(err));

type PageState =
  | { status: 'loading' }
  | { status: 'ready'; content: RenderContent }
  | { status: 'error'; message: string };

function useRenderStorages(): RenderStorages {
  const testStorages = useTestStorages();
  return useMemo(
    () => ({
      maps: testStorages?.mapStorage ?? new FilesystemMapStorage(studioApiOptions(paths.maps)),
      rooms: testStorages?.roomStorage ?? new FilesystemRoomStorage(studioApiOptions(paths.rooms)),
      layouts: testStorages?.layoutStorage ?? new FilesystemLayoutStorage(studioApiOptions(paths.layouts)),
    }),
    [testStorages],
  );
}

/**
 * Draws one stored map, room or layout from the requested camera.
 *
 * Mounted by HeadlessApp on `?op=render`. `window.__officexrRenderReady`
 * is set once every model and baked GLB has loaded and a few frames have
 * rendered; `window.__officexrRenderError` is set instead when the
 * parameters, the document or an asset are bad.
 */
export function RenderPage() {
  const api = useApplication();
  const storages = useRenderStorages();
  const parsed = useMemo(() => parseRenderParams(new URLSearchParams(window.location.search)), []);
  const [state, setState] = useState<PageState>({ status: 'loading' });

  // The sentinel is set here, not from a render, so the ready signal can never race past an error.
  const fail = useCallback((message: string) => {
    const w = sentinels();
    w.__officexrRenderError ??= message;
    setState({ status: 'error', message: w.__officexrRenderError });
  }, []);

  useEffect(() => {
    if (!parsed.ok) {
      fail(parsed.error);
      return;
    }
    let cancelled = false;
    loadRenderContent(api, storages, parsed.params).then(
      (content) => !cancelled && setState({ status: 'ready', content }),
      (err) => !cancelled && fail(errorMessage(err)),
    );
    return () => {
      cancelled = true;
    };
  }, [api, storages, parsed, fail]);
  useEffect(
    () => () => {
      delete sentinels().__officexrRenderReady;
      delete sentinels().__officexrRenderError;
    },
    [],
  );

  if (state.status === 'error') {
    return <pre style={{ color: '#fcc', fontFamily: 'monospace', padding: 16 }}>{state.message}</pre>;
  }
  if (state.status === 'loading' || !parsed.ok) {
    return null;
  }
  return <RenderView params={parsed.params} content={state.content} onError={fail} />;
}

function RenderView({
  params,
  content,
  onError,
}: {
  params: RenderParams;
  content: RenderContent;
  onError: (message: string) => void;
}) {
  const api = useApplication();
  const container = useRef<HTMLDivElement>(null);
  const bevy = usePrimaryBevyView(container, (e) => onError(`renderer failed to load: ${e.message}`));
  const setup = useMemo(() => frameCamera(params.camera, content.bounds, params), [params, content.bounds]);

  useEffect(() => {
    if (!bevy) return;
    const { wasm, view } = bevy;
    let cancelled = false;
    const blobs: string[] = [];
    const world = renderWorld(api, content);
    void (async () => {
      // Bevy never reports a model that fails to load, so a missing one is caught here first. Baked
      // layouts go to Bevy as blobs, since the studio API wants credentials Bevy's fetch doesn't send.
      const [bakedLayouts] = await Promise.all([
        Promise.all(
          (world.bakedLayouts ?? []).map(async (b) => {
            const url = URL.createObjectURL(await (await fetchOk(b.gltfPath, studioFetch)).blob());
            blobs.push(url);
            return { ...b, gltfPath: blobAssetPath(url, 'glb') };
          }),
        ),
        Promise.all(content.modelUrls.map((url) => fetchOk(url, fetch))),
      ]);
      if (cancelled) {
        return;
      }
      wasm.setWorld(view, JSON.stringify({ ...world, bakedLayouts }));
      wasm.setOverlays(view, [GRID]);
      wasm.setCamera(view, cameraPose(setup, params));
      // A capture resolves on a drawn frame, so readiness is read once the diff has been applied,
      // and the frame after it has the loaded models in it.
      await wasm.captureView(view);
      while (!cancelled && !wasm.isReady(view).ready) {
        await new Promise((resolve) => setTimeout(resolve, READY_POLL_MS));
      }
      if (!cancelled) {
        await wasm.captureView(view);
        reportReady(content.bounds, setup);
      }
    })().catch((err: unknown) => !cancelled && onError(errorMessage(err)));
    return () => {
      cancelled = true;
      blobs.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [bevy, api, content, setup, params, onError]);

  return (
    <div
      ref={container}
      data-render-target={params.target}
      style={{ position: 'absolute', top: 0, left: 0, width: params.width, height: params.height }}
    />
  );
}

async function fetchOk(url: string, fetcher: typeof fetch): Promise<Response> {
  const response = await fetcher(url).catch((err: unknown) => err);
  if (!(response instanceof Response) || !response.ok) {
    const why = response instanceof Response ? `HTTP ${response.status}` : errorMessage(response);
    throw new Error(`failed to load asset: ${url} (${why})`);
  }
  return response;
}

/** The content as one world diff, under the editors' lighting on a flat backdrop. */
function renderWorld(api: ApplicationApi, content: RenderContent): WorldDiff {
  const placed: RoomPlacements =
    content.type === 'map'
      ? content.placed
      : {
          instances: content.worldObjects.instances,
          bakedLayouts:
            content.type === 'room' && content.layoutName
              ? [{ gltfPath: studioBakedLayoutUrl(content.layoutName), position: [0, 0, 0], rotationY: 0, layoutName: content.layoutName }]
              : [],
        };
  const kindIds = new Set(placed.instances.map((i) => i.kindId));
  return {
    replace: true,
    kinds: api.catalog.getCatalog().kinds.filter((k) => kindIds.has(k.id)),
    objects: { instances: placed.instances },
    bakedLayouts: placed.bakedLayouts,
    lighting: DEFAULT_EDITOR_LIGHTING,
    background: DEFAULT_WORLD_CONSTANTS.background,
    clearColor: BACKDROP,
  };
}

function reportReady(bounds: Bounds, setup: CameraSetup) {
  const w = sentinels();
  if (w.__officexrRenderError === undefined) {
    w.__officexrRenderReady = {
      bounds,
      metresPerPixel: setup.type === 'orthographic' ? setup.metresPerPixel : undefined,
    };
  }
}
