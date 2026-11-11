import { CHROME_APP_DEVICE, type AudioDevice } from '@/types/voice';

export const CHROME_APP_LABEL = 'Google Chrome app audio (Google Voice)';

/** Selection value sent to Rust: the device name plus its direction, e.g. "Headset (Jabra) (input)". */
export function deviceValue(device: AudioDevice): string {
  return `${device.name} (${device.device_type.toLowerCase()})`;
}

export function deviceLabel(device: AudioDevice): string {
  if (device.name === CHROME_APP_DEVICE) return CHROME_APP_LABEL;
  return device.label || device.name;
}

/** Label of a stored selection value ("<name> (input|output)"); "Default" when none. */
export function selectionLabel(value: string | null): string {
  if (!value) return 'Default';
  const name = value.replace(/ \((input|output)\)$/, '');
  return name === CHROME_APP_DEVICE ? CHROME_APP_LABEL : name;
}

export function inputDevices(devices: AudioDevice[]): AudioDevice[] {
  return devices.filter(d => d.device_type === 'Input');
}

/** System sources with the Chrome app pseudo device first. */
export function systemDevices(devices: AudioDevice[]): AudioDevice[] {
  return devices
    .filter(d => d.device_type === 'Output')
    .sort((a, b) => Number(b.name === CHROME_APP_DEVICE) - Number(a.name === CHROME_APP_DEVICE));
}
