import { useEffect, useRef, useSyncExternalStore } from 'react';
import type { InteractionPrompt, InteractionStore } from '../input/interaction.ts';
import '../renderer/InteractionPromptAnchor.css';
import { useCoarsePointer } from './useCoarsePointer.ts';
import type { BevyView } from './usePrimaryBevyView.ts';

/** The pill sits this far above the object's anchor. */
const RAISE_M = 0.1;

/**
 * The interaction options (E Sit, F Ride) next to the object they act on: a pill kept, every
 * frame, where the view's camera sees the prompt's anchor, and hidden while the anchor is behind
 * the camera. Touch screens show the options on the action pad instead.
 */
export function InteractionPromptAnchor({ interactions, bevy }: { interactions: InteractionStore; bevy: BevyView | null }) {
  const prompt = useSyncExternalStore(interactions.subscribe, interactions.get);
  const coarse = useCoarsePointer();
  const node = useRef<HTMLDivElement>(null);
  const { anchor } = prompt;
  const shown = !coarse && anchor !== null && bevy !== null;

  useEffect(() => {
    if (!shown || anchor === null || bevy === null) {
      return;
    }
    const view = bevy;
    const point: [number, number, number] = [anchor.x, anchor.y + RAISE_M, anchor.z];
    let frame = 0;
    const place = () => {
      const el = node.current;
      if (el) {
        const at = projectSafely(view, point);
        el.style.visibility = at ? 'visible' : 'hidden';
        if (at) {
          el.style.transform = `translate(${at[0]}px, ${at[1]}px) translate(-50%, -50%)`;
        }
      }
      frame = requestAnimationFrame(place);
    };
    place();
    return () => cancelAnimationFrame(frame);
  }, [shown, bevy, anchor]);

  if (!shown) {
    return null;
  }
  return (
    <div
      ref={node}
      style={{ position: 'absolute', left: 0, top: 0, pointerEvents: 'none', visibility: 'hidden', zIndex: 1 }}
    >
      <InteractionPill options={prompt.options} />
    </div>
  );
}

/** Null until the view has drawn a camera, which `projectToScreen` throws for. */
function projectSafely(bevy: BevyView, point: [number, number, number]): [number, number] | null {
  try {
    return bevy.wasm.projectToScreen(bevy.view, point);
  } catch {
    return null;
  }
}

export function InteractionPill({ options }: Pick<InteractionPrompt, 'options'>) {
  return (
    <div className="interaction-pill" role="status" aria-label="Interactions">
      {options.map((o) => (
        <span key={o.kind} className="interaction-pill-option">
          <kbd className="interaction-pill-key">{o.key}</kbd>
          {o.label}
        </span>
      ))}
    </div>
  );
}
