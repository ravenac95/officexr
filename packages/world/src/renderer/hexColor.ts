const HSL = /^hsl\(\s*(-?[\d.]+)(?:deg)?\s*[,\s]\s*([\d.]+)%\s*[,\s]\s*([\d.]+)%\s*\)$/i;

const byte = (v: number) =>
  Math.round(Math.max(0, Math.min(1, v)) * 255)
    .toString(16)
    .padStart(2, '0');

/**
 * `#rrggbb` for a `#rgb`, `#rrggbb` or `hsl(h, s%, l%)` colour, as the renderer takes colours; `null`
 * for anything else. Spells and player accents are written as `hsl()`.
 */
export function hexColor(css: string): string | null {
  const s = css.trim();
  if (/^#[\da-f]{6}$/i.test(s)) {
    return s.toLowerCase();
  }
  if (/^#[\da-f]{3}$/i.test(s)) {
    return `#${[...s.slice(1)].map((c) => c + c).join('')}`.toLowerCase();
  }
  const hsl = HSL.exec(s);
  if (!hsl) {
    return null;
  }
  const h = (((Number(hsl[1]) % 360) + 360) % 360) / 360;
  const sat = Number(hsl[2]) / 100;
  const l = Number(hsl[3]) / 100;
  const q = l < 0.5 ? l * (1 + sat) : l + sat - l * sat;
  const p = 2 * l - q;
  const channel = (t0: number) => {
    const t = t0 < 0 ? t0 + 1 : t0 > 1 ? t0 - 1 : t0;
    return t < 1 / 6 ? p + (q - p) * 6 * t : t < 1 / 2 ? q : t < 2 / 3 ? p + (q - p) * (2 / 3 - t) * 6 : p;
  };
  return `#${byte(channel(h + 1 / 3))}${byte(channel(h))}${byte(channel(h - 1 / 3))}`;
}
