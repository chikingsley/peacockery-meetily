import { expect, test } from "bun:test";

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";

import { captionLanguage, captionSource, englishCaption } from "./captions";

test("Chinese turns keep Latin part numbers and share one translation across views", async () => {
  const text = "Schedule 40 的配件。";
  const requests: unknown[] = [];

  const previous = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {},
  });

  mockIPC((_cmd, args) => {
    requests.push(args);

    return "Schedule 40 fittings.";
  });

  try {
    const live = englishCaption(text, true);
    const replay = englishCaption(text, false);

    expect(replay).toBe(live);
    expect(await replay).toBe("Schedule 40 fittings.");
    expect(requests).toEqual([{ language: "zh", text }]);
    expect(englishCaption("Please quote Schedule 40.", true)).toBeNull();
    expect(captionLanguage("見積もり")).toBe("ja");
    expect(captionLanguage("견적")).toBe("ko");
  } finally {
    clearMocks();

    if (previous) {
      Object.defineProperty(globalThis, "window", previous);
    } else {
      Reflect.deleteProperty(globalThis, "window");
    }
  }
});

test("mixed turns translate the Chinese sentences and keep embedded part numbers", () => {
  expect(captionSource("[软件铃声] Hello, Jerry.")).toBe("[软件铃声]");
  expect(captionSource("你好。You're muted.")).toBe("你好。");
  expect(captionSource("Hello! 请确认 Schedule 40 的配件。")).toBe(
    "请确认 Schedule 40 的配件。"
  );
});
