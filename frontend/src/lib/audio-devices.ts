import { CHROME_APP_DEVICE } from "@/types/voice";
import type { AudioDevice } from "@/types/voice";

/** Selection value sent to Rust: the device name plus its direction, e.g. "Headset (Jabra) (input)". */
export function deviceValue(device: AudioDevice): string {
  return `${device.name} (${device.device_type.toLowerCase()})`;
}

/** Display name: the label Rust sends for pseudo devices such as `app:chrome`, else the name. */
export function deviceLabel(device: AudioDevice): string {
  return device.label || device.name;
}

/** Label of a stored selection value ("<name> (input|output)"); "Default" when none. */
export function selectionLabel(
  value: string | null,
  devices: AudioDevice[]
): string {
  if (!value) {
    return "Default";
  }

  const device = devices.find((d) => deviceValue(d) === value);

  return device
    ? deviceLabel(device)
    : value.replace(/ \((input|output)\)$/, "");
}

export function inputDevices(devices: AudioDevice[]): AudioDevice[] {
  return devices.filter((d) => d.device_type === "Input");
}

/** System sources with the Chrome app pseudo device first. */
export function systemDevices(devices: AudioDevice[]): AudioDevice[] {
  return devices
    .filter((d) => d.device_type === "Output")
    .toSorted(
      (a, b) =>
        Number(b.name === CHROME_APP_DEVICE) -
        Number(a.name === CHROME_APP_DEVICE)
    );
}
