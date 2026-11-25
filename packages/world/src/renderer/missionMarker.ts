/** Where the active mission points: a spot on the floor and its label. */
export type MissionMarkerTarget = { x: number; z: number; label: string };

/** The beacon's caption: the label, plus whole metres to go when the player's position is known. */
export const markerText = (label: string, metres: number | null) =>
  metres === null ? label : `${label} · ${metres < 1 ? '<1' : Math.floor(metres)} m`;
