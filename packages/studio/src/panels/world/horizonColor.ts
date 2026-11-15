/** Where the two-stop gradient sits at the horizon: smoothstep(-0.2, 1, 0), as in the renderer's sky. */
const TWO_STOP_HORIZON_T = 0.0741;

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toSrgb = (c: number) => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);

function linearChannels(hex: string): number[] {
  const n = parseInt(hex.replace('#', ''), 16);
  return [16, 8, 0].map((shift) => toLinear(((n >> shift) & 0xff) / 255));
}

/** The `#rrggbb` the sky shows at the horizon when no horizon colour is set: the two-stop gradient there, mixed in linear light. */
export function twoStopHorizonColor(topColor: string, bottomColor: string): string {
  const top = linearChannels(topColor);
  const bottom = linearChannels(bottomColor);
  return (
    '#' +
    bottom
      .map((b, i) => b + (top[i] - b) * TWO_STOP_HORIZON_T)
      .map((c) => Math.round(toSrgb(c) * 255).toString(16).padStart(2, '0'))
      .join('')
  );
}
