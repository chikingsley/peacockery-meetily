import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";

const subscribe = () => () => {
  /* Runtime identity stays fixed for the window. */
};

const serverSnapshot = () => false;

/** Match the static export during hydration, then expose native capabilities. */
export function useDesktopRuntime() {
  return useSyncExternalStore(subscribe, isTauri, serverSnapshot);
}
