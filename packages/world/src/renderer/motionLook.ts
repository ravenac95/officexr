export interface MotionLookState {
  enabled: boolean;
  /** Changing this restores the view captured when motion look was enabled. */
  recenterKey: number;
}

export type MotionPermission = 'granted' | 'denied' | 'unsupported';

const PROBE_TIMEOUT_MS = 500;

/**
 * Asks for motion access and checks the device really reports it. iOS needs
 * `requestPermission()` called synchronously inside a tap, so it is the first
 * thing this does. Desktop Chrome exposes `DeviceOrientationEvent` but never
 * fires it, hence the wait for a first reading.
 */
export async function requestMotionPermission(): Promise<MotionPermission> {
  if (typeof DeviceOrientationEvent === 'undefined') {
    return 'unsupported';
  }
  const { requestPermission } = DeviceOrientationEvent as { requestPermission?: () => Promise<string> };
  if (typeof requestPermission === 'function') {
    try {
      if ((await requestPermission.call(DeviceOrientationEvent)) !== 'granted') {
        return 'denied';
      }
    } catch {
      return 'denied';
    }
  }
  return new Promise((resolve) => {
    const finish = (result: MotionPermission) => {
      clearTimeout(timer);
      window.removeEventListener('deviceorientation', onReading);
      resolve(result);
    };
    const onReading = (e: DeviceOrientationEvent) => {
      if (e.alpha !== null && e.beta !== null && e.gamma !== null) {
        finish('granted');
      }
    };
    const timer = setTimeout(() => finish('unsupported'), PROBE_TIMEOUT_MS);
    window.addEventListener('deviceorientation', onReading);
  });
}
