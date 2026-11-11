'use client';

import { useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';

/** The app version from the Tauri config (single source of truth). */
export function AppVersion({ className }: { className?: string }) {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    getVersion().then(setVersion).catch((error) => console.error('getVersion failed:', error));
  }, []);
  if (!version) return null;
  return <div className={className}>v{version}</div>;
}
