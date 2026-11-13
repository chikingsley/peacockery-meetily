"use client";

import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { Separator } from "@/components/ui/separator";
import { SidebarInset, SidebarTrigger } from "@/components/ui/sidebar";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { useDesktopRuntime } from "@/hooks/useDesktopRuntime";

export default function MainContent({ children }: { children: ReactNode }) {
  const path = usePathname();
  const desktop = useDesktopRuntime();
  const { currentMeeting } = useSidebar();
  const { isRecording } = useRecordingState();

  const title =
    path === "/settings"
      ? "Settings"
      : path === "/meeting-details"
        ? (currentMeeting?.title ?? "Meeting")
        : isRecording
          ? "Recording workspace"
          : "New meeting";

  return (
    <SidebarInset className="h-svh overflow-hidden">
      <header className="flex h-14 shrink-0 items-center gap-3 border-b px-4">
        <SidebarTrigger />
        <Separator orientation="vertical" className="h-4" />
        <span className="truncate text-sm font-medium">{title}</span>
      </header>
      {!desktop && (
        <p className="border-b px-4 py-2 text-xs text-muted-foreground">
          Browser preview · Open Peacockery Meetily on your PC to record and
          access saved meetings and settings.
        </p>
      )}
      <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
    </SidebarInset>
  );
}
