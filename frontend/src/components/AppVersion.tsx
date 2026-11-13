"use client";

import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import { version as buildVersion } from "../../package.json";

/** The app version from the Tauri config (single source of truth). */
export function AppVersion({ className }: { className?: string }) {
  const [version, setVersion] = useState<string | null>(buildVersion);
  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    getVersion()
      .then(setVersion)
      .catch((error) => console.error("getVersion failed:", error));
  }, []);

  if (!version) {
    return null;
  }

  return <div className={className}>v{version}</div>;
}
