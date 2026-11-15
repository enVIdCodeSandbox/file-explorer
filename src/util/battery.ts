import { useEffect, useState } from "react";

/** A phone or camera on a cable. */
export const isDevicePath = (p: string) => p.slice(0, 6).toLowerCase() === "mtp://";

/** Below this charge, on battery, optional background work waits. */
export const LOW_BATTERY = 0.2;

export const isLowBattery = (b: { charging: boolean; level: number } | null | undefined) => !!b && !b.charging && b.level < LOW_BATTERY;

interface BatteryLike extends EventTarget {
  charging: boolean;
  level: number;
}

/** True while this computer is on low battery. Always false where the system does not say. */
export function useThriftyBattery(enabled: boolean): boolean {
  const [low, setLow] = useState(false);
  useEffect(() => {
    if (!enabled) return setLow(false);
    const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryLike> };
    if (!nav.getBattery) return;
    let gone = false;
    let battery: BatteryLike | undefined;
    const update = () => battery && !gone && setLow(isLowBattery(battery));
    nav
      .getBattery()
      .then((b) => {
        battery = b;
        update();
        b.addEventListener("chargingchange", update);
        b.addEventListener("levelchange", update);
      })
      .catch(() => {});
    return () => {
      gone = true;
      battery?.removeEventListener("chargingchange", update);
      battery?.removeEventListener("levelchange", update);
    };
  }, [enabled]);
  return low;
}
