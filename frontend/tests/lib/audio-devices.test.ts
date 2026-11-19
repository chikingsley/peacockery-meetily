import { describe, expect, test } from "bun:test";

import {
  connectedSelection,
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
  test("Start records on a selected device while it is connected, else on the default", () => {
    const jabra = "Headset (Jabra Evolve2 65) (input)";
    expect(connectedSelection(jabra, devices)).toBe(jabra);
    expect(connectedSelection(jabra, devices.slice(0, 1))).toBeNull();
    expect(connectedSelection(null, devices)).toBeNull();
  });

  test("selection values carry the direction", () => {
    expect(deviceValue(devices[1])).toBe("Headset (Jabra Evolve2 65) (input)");
    expect(deviceValue(devices[2])).toBe("app:chrome (output)");
  });

  test("the Chrome pseudo device shows the label Rust sends and is listed first", () => {
    expect(deviceLabel(devices[2])).toBe(
      "Google Chrome app audio (Google Voice)"
    );
    expect(deviceLabel(devices[0])).toBe("Speakers (Realtek)");
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
    expect(selectionLabel(null, devices)).toBe("Default");
    expect(selectionLabel("app:chrome (output)", devices)).toBe(
      "Google Chrome app audio (Google Voice)"
    );
    expect(selectionLabel("Headset (Jabra Evolve2 65) (input)", devices)).toBe(
      "Headset (Jabra Evolve2 65)"
    );
    // A device that is no longer listed keeps its stored name.
    expect(selectionLabel("Old USB Mic (input)", devices)).toBe("Old USB Mic");
  });
});
