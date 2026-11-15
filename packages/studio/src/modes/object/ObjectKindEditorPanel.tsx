import React from 'react';
import {
  CUBE_KIND_CATEGORIES,
  type CubeKindCategory,
  type OptimizationMode,
  type SeatSpec,
  type WorldObjectKind,
} from '@officexr/world';

import { Button } from '../../components/ui/button.tsx';
import { bakedSize, type KindSize } from './bakedSize.ts';
import {
  ColorInput,
  NullableField,
  NumberInput,
  Panel,
  Readonly,
  Section,
  SelectInput,
  Toggle,
} from '../../ui/controls/index.ts';

interface ObjectKindEditorPanelProps {
  kind: WorldObjectKind | null;
  applyPatch: (partial: Partial<WorldObjectKind>) => void;
}

/**
 * Right-hand panel for the Object editor. Replaces the previous
 * `useKindEditor` Leva hook + its workarounds for Leva's mount-fire
 * bug (`info.initial` guards and the imperative `setLeva({...})`
 * push — see git history for the gory details).
 *
 * Every field is a fully-controlled React input. `kind` comes in
 * from `useObjectCatalog`; every edit fires `applyPatch(partial)`
 * which routes through the existing no-op guard in
 * `useObjectCatalog.applyPatch` to the catalog mutator. The bug
 * that motivated this whole migration becomes structurally
 * impossible: there is no shared store between the previous kind
 * and the new kind to leak across.
 */
export function ObjectKindEditorPanel({ kind, applyPatch }: ObjectKindEditorPanelProps) {
  if (!kind) {
    return (
      <Panel>
        <div className="px-3 py-3 text-xs text-muted-foreground">
          Pick a kind on the left to edit its label, swatch, walkable
          flag, scale, and material overrides.
        </div>
      </Panel>
    );
  }

  return (
    <Panel>
      <Section title="Kind">
        <Readonly label="id" value={kind.id} />
        <LabelInput
          value={kind.label}
          onChange={(label) => applyPatch({ label })}
        />
        <SelectInput
          label="category"
          value={kind.category}
          options={CUBE_KIND_CATEGORIES as unknown as readonly CubeKindCategory[]}
          onChange={(category) => applyPatch({ category })}
        />
        <ColorInput
          label="swatch"
          value={kind.swatch}
          onChange={(swatch) => applyPatch({ swatch })}
        />
        <Toggle
          label="walkable"
          value={kind.walkable}
          onChange={(walkable) => applyPatch({ walkable })}
        />
        <NumberInput
          label="scale"
          value={kind.scale}
          min={0.1}
          max={4}
          step={0.05}
          onChange={(scale) => applyPatch({ scale })}
        />
      </Section>

      <Section title="Dimensions">
        <KindDimensionsEditor kind={kind} applyPatch={applyPatch} />
      </Section>

      <Section title="Material overrides">
        <NullableField<string>
          label="tint"
          value={kind.tint}
          defaultValue="#ffffff"
          onChange={(tint) => applyPatch({ tint })}
        >
          {(value, onChange) => (
            <ColorInput label="" value={value} onChange={onChange} />
          )}
        </NullableField>
        <NumberInput
          label="opacity"
          value={kind.opacity}
          min={0}
          max={1}
          step={0.05}
          onChange={(opacity) => applyPatch({ opacity })}
        />
        <NullableField<number>
          label="roughness"
          value={kind.roughness}
          defaultValue={0.5}
          onChange={(roughness) => applyPatch({ roughness })}
        >
          {(value, onChange) => (
            <NumberInput
              label=""
              value={value}
              min={0}
              max={1}
              step={0.05}
              onChange={onChange}
            />
          )}
        </NullableField>
        <NullableField<number>
          label="metalness"
          value={kind.metalness}
          defaultValue={0}
          onChange={(metalness) => applyPatch({ metalness })}
        >
          {(value, onChange) => (
            <NumberInput
              label=""
              value={value}
              min={0}
              max={1}
              step={0.05}
              onChange={onChange}
            />
          )}
        </NullableField>
        <NullableField<string>
          label="emissive"
          value={kind.emissive}
          defaultValue="#000000"
          onChange={(emissive) => applyPatch({ emissive })}
        >
          {(value, onChange) => (
            <ColorInput label="" value={value} onChange={onChange} />
          )}
        </NullableField>
        <NumberInput
          label="emissive intensity"
          value={kind.emissiveIntensity}
          min={0}
          max={4}
          step={0.1}
          onChange={(emissiveIntensity) => applyPatch({ emissiveIntensity })}
        />
      </Section>

      <Section title="Tiling">
        <Toggle
          label="tile X"
          value={kind.tilingAxes.x}
          onChange={(v) => applyPatch({ tilingAxes: { ...kind.tilingAxes, x: v } })}
        />
        <Toggle
          label="tile Y"
          value={kind.tilingAxes.y}
          onChange={(v) => applyPatch({ tilingAxes: { ...kind.tilingAxes, y: v } })}
        />
        <Toggle
          label="tile Z"
          value={kind.tilingAxes.z}
          onChange={(v) => applyPatch({ tilingAxes: { ...kind.tilingAxes, z: v } })}
        />
      </Section>

      <Section title="Placement">
        <Toggle
          label="gravity"
          value={kind.gravity}
          onChange={(gravity) => applyPatch({ gravity })}
        />
        <Toggle
          label="Is layout object"
          value={kind.isLayoutObject}
          onChange={(isLayoutObject) => applyPatch({ isLayoutObject })}
          hint="Walls, floors, structural geometry. Used in Layout view; hidden from Room view by default."
        />
      </Section>

      <Section title="Seating">
        <Toggle
          label="sittable"
          value={kind.seat !== undefined}
          onChange={(sittable) => applyPatch({ seat: sittable ? defaultSeat(kind) : undefined })}
          hint="Offset is in metres from the model's min corner, unrotated: the seated avatar's root rests on the seat surface, so y is the cushion top."
        />
        {kind.seat && (
          <SeatFields seat={kind.seat} onChange={(seat) => applyPatch({ seat })} />
        )}
        <Toggle
          label="is desk"
          value={kind.isDesk ?? false}
          onChange={(isDesk) => applyPatch({ isDesk: isDesk || undefined })}
        />
      </Section>

      <Section title="Optimization">
        <SelectInput
          label="mode"
          value={kind.optimization}
          options={['none', 'static-batch', 'frustum-cull'] as readonly OptimizationMode[]}
          onChange={(optimization) => applyPatch({ optimization })}
        />
        <Readonly label="" value="scaffold — no runtime effect" />
      </Section>
    </Panel>
  );
}

function defaultSeat(kind: WorldObjectKind): SeatSpec {
  const { width = 0, height = 0, depth = 0 } = kind.dimensions ?? {};
  return { offset: { x: width / 2, y: height * 0.45, z: depth / 2 }, yawDeg: 0 };
}

function SeatFields({ seat, onChange }: { seat: SeatSpec; onChange: (seat: SeatSpec) => void }) {
  const axes = ['x', 'y', 'z'] as const;
  return (
    <>
      {axes.map((axis) => (
        <NumberInput
          key={axis}
          label={`offset ${axis}`}
          value={seat.offset[axis]}
          step={0.05}
          onChange={(v) => onChange({ ...seat, offset: { ...seat.offset, [axis]: v } })}
        />
      ))}
      <NumberInput
        label="yaw"
        value={seat.yawDeg}
        step={15}
        onChange={(yawDeg) => onChange({ ...seat, yawDeg })}
      />
    </>
  );
}

/** @deprecated Use ObjectKindEditorPanel */
export const KindEditorPanel = ObjectKindEditorPanel;

interface LabelInputProps {
  value: string;
  onChange: (next: string) => void;
}

interface KindDimensionsEditorProps {
  kind: WorldObjectKind;
  applyPatch: (partial: Partial<WorldObjectKind>) => void;
}

/**
 * Width / height / depth editor backed by the kind's `dimensions` in the catalog. Unset, it shows the
 * size of the bounds `pnpm --filter @officexr/world bake:dimensions` measured without writing it;
 * "Reset to baked bounds" commits that size. A kind with neither has nothing to edit from.
 */
function KindDimensionsEditor({ kind, applyPatch }: KindDimensionsEditorProps) {
  const baked = bakedSize(kind);
  const effective = kind.dimensions ?? baked;
  const axes = ['width', 'height', 'depth'] as const;
  if (!effective) {
    return (
      <div>
        {axes.map((axis) => (
          <Readonly key={axis} label={axis} value="—" />
        ))}
        <p className="px-3 py-1 text-xs text-muted-foreground">
          Not baked: run <code>pnpm --filter @officexr/world bake:dimensions</code>.
        </p>
      </div>
    );
  }

  const update = (axis: keyof KindSize, value: number) => {
    if (value <= 0) return;
    applyPatch({ dimensions: { ...effective, [axis]: value } });
  };

  return (
    <div>
      {axes.map((axis) => (
        <NumberInput
          key={axis}
          label={axis}
          value={effective[axis]}
          min={0.05}
          max={64}
          step={0.05}
          onChange={(v) => update(axis, v)}
        />
      ))}
      <div className="flex items-center justify-between gap-2 px-3 py-1">
        <span className="text-xs text-muted-foreground">
          {kind.dimensions ? 'set' : 'unset (showing the baked bounds)'}
        </span>
        <Button
          variant="secondary"
          size="sm"
          disabled={!baked}
          onClick={() => baked && applyPatch({ dimensions: baked })}
          title="Overwrite width/height/depth with the size of the baked bounds, at the current scale."
        >
          Reset to baked bounds
        </Button>
      </div>
    </div>
  );
}

/** Tiny inline text field for the "label" row. Kept as a separate
 * component so the Field row matches the rest of the panel's
 * spacing without duplicating the label-cell markup. */
function LabelInput({ value, onChange }: LabelInputProps) {
  return (
    <div className="flex items-center gap-2 px-3 py-1 min-h-7">
      <label className="w-[35%] shrink-0 text-[11px] text-muted-foreground">
        label
      </label>
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        spellCheck={false}
        className="flex-1 min-w-0 rounded-sm border border-input bg-secondary px-2 py-0.5 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
      />
    </div>
  );
}
