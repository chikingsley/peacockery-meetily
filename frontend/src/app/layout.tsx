"use client";

import "./globals.css";
import "sonner/dist/styles.css";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { ThemeProvider } from "next-themes";
import { Source_Sans_3 } from "next/font/google";
import { useState, useEffect, useCallback } from "react";
import { Toaster, toast } from "sonner";

import { ImportAudioDialog, ImportDropOverlay } from "@/components/ImportAudio";
import MainContent from "@/components/MainContent";
import Sidebar from "@/components/Sidebar";
import { SidebarProvider } from "@/components/Sidebar/SidebarProvider";
import { SidebarProvider as ShellSidebarProvider } from "@/components/ui/sidebar";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  isAudioExtension,
  getAudioFormatsDisplayList,
} from "@/constants/audioFormats";
import { ConfigProvider } from "@/contexts/ConfigContext";
import { ImportDialogProvider } from "@/contexts/ImportDialogContext";
import { RecordingStateProvider } from "@/contexts/RecordingStateContext";
import { RecordingStopProvider } from "@/contexts/RecordingStopContext";
import { TranscriptProvider } from "@/contexts/TranscriptContext";

const sourceSans3 = Source_Sans_3({
  subsets: ["latin"],
  variable: "--font-source-sans-3",
  weight: ["400", "500", "600", "700"],
});

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const [showDropOverlay, setShowDropOverlay] = useState(false);
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [importFilePath, setImportFilePath] = useState<string | null>(null);

  // Disable the browser context menu in production builds.
  useEffect(() => {
    if (process.env.NODE_ENV !== "production") {
      return;
    }

    const handleContextMenu = (e: MouseEvent) => e.preventDefault();
    document.addEventListener("contextmenu", handleContextMenu);

    return () => document.removeEventListener("contextmenu", handleContextMenu);
  }, []);

  const openImportDialog = useCallback((filePath?: string | null) => {
    setImportFilePath(filePath ?? null);
    setShowImportDialog(true);
  }, []);

  const handleFileDrop = useCallback(
    (paths: string[]) => {
      const audioFile = paths.find((p) => {
        const ext = p.split(".").pop()?.toLowerCase();

        return !!ext && isAudioExtension(ext);
      });

      if (audioFile) {
        openImportDialog(audioFile);
      } else if (paths.length > 0) {
        toast.error("Please drop an audio file", {
          description: `Supported formats: ${getAudioFormatsDisplayList()}`,
        });
      }
    },
    [openImportDialog]
  );

  // Drag-and-drop audio import.
  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    const keep = (fn: UnlistenFn) => {
      if (cancelled) {
        fn();
      } else {
        unlisteners.push(fn);
      }
    };

    (async () => {
      keep(await listen("tauri://drag-enter", () => setShowDropOverlay(true)));
      keep(await listen("tauri://drag-leave", () => setShowDropOverlay(false)));
      keep(
        await listen<{ paths: string[] }>("tauri://drag-drop", (event) => {
          setShowDropOverlay(false);
          handleFileDrop(event.payload.paths);
        })
      );
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [handleFileDrop]);

  const handleImportDialogChange = useCallback((open: boolean) => {
    setShowImportDialog(open);

    if (!open) {
      setImportFilePath(null);
    }
  }, []);

  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <title>Peacockery Meetily</title>
      </head>
      <body className={`${sourceSans3.variable} font-sans antialiased`}>
        <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
          <RecordingStateProvider>
            <TranscriptProvider>
              <ConfigProvider>
                <SidebarProvider>
                  <RecordingStopProvider>
                    <TooltipProvider>
                      <ImportDialogProvider onOpen={openImportDialog}>
                        <ShellSidebarProvider>
                          <Sidebar />
                          <MainContent>{children}</MainContent>
                        </ShellSidebarProvider>
                        <ImportDropOverlay visible={showDropOverlay} />
                        <ImportAudioDialog
                          open={showImportDialog}
                          onOpenChange={handleImportDialogChange}
                          preselectedFile={importFilePath}
                        />
                      </ImportDialogProvider>
                    </TooltipProvider>
                  </RecordingStopProvider>
                </SidebarProvider>
              </ConfigProvider>
            </TranscriptProvider>
          </RecordingStateProvider>
        </ThemeProvider>
        <Toaster position="bottom-center" richColors closeButton />
      </body>
    </html>
  );
}
