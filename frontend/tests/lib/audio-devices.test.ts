import { describe, expect, test } from "bun:test";

import {
  CHROME_APP_LABEL,
  deviceLabel,
  deviceValue,
  inputDevices,
  selectionLabel,
  systemDevices,
} from "../../src/lib/audio-devices";
import type { AudioDevice } from "../../src/types/voice";

const devices: AudioDevice[] = [
  { device_type: "Output", name: "Speakers (Realtek)" },
  { device_type: "Input", name: "Headset (Jabra Evolve2 65)" },
  {
    device_type: "Output",
    label: "Google Chrome app audio (Google Voice)",
    name: "app:chrome",
  },
  { device_type: "Output", name: "Headset (Jabra Evolve2 65)" },
];

describe("audio devices", () => {
  test("selection values carry the direction", () => {
    expect(deviceValue(devices[1])).toBe("Headset (Jabra Evolve2 65) (input)");
    expect(deviceValue(devices[2])).toBe("app:chrome (output)");
  });

  test("the Chrome pseudo device is shown by name and listed first", () => {
    expect(deviceLabel({ device_type: "Output", name: "app:chrome" })).toBe(
      CHROME_APP_LABEL
    );
    expect(systemDevices(devices).map((d) => d.name)).toEqual([
      "app:chrome",
      "Speakers (Realtek)",
      "Headset (Jabra Evolve2 65)",
    ]);
    expect(inputDevices(devices).map((d) => d.name)).toEqual([
      "Headset (Jabra Evolve2 65)",
    ]);
  });

  test("labels stored selections", () => {
    expect(selectionLabel(null)).toBe("Default");
    expect(selectionLabel("app:chrome (output)")).toBe(CHROME_APP_LABEL);
    expect(selectionLabel("Headset (Jabra Evolve2 65) (input)")).toBe(
      "Headset (Jabra Evolve2 65)"
    );
  });
});
