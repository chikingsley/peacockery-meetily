"use client";

import { FileText, Sparkles } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

const STORAGE_KEY = "peacockery-meetily.meetingDetails.transcriptPaneRatio";
const DEFAULT_RATIO = 0.5;
const MIN_RATIO = 0.3;
const MAX_RATIO = 0.5;

const clampRatio = (value: number) =>
  Math.min(MAX_RATIO, Math.max(MIN_RATIO, value));

const TABS = [
  { icon: FileText, label: "Transcript", value: "transcript" as const },
  { icon: Sparkles, label: "Summary", value: "summary" as const },
];

function readStoredRatio(): number {
  if (typeof window === "undefined") {
    return DEFAULT_RATIO;
  }

  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const n = raw === null ? Number.NaN : Number(raw);

    return Number.isFinite(n) && n >= MIN_RATIO && n <= MAX_RATIO
      ? n
      : DEFAULT_RATIO;
  } catch {
    return DEFAULT_RATIO;
  }
}

function writeStoredRatio(value: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(value));
  } catch {
    // Layout persistence is optional.
  }
}

export type MeetingDetailsTab = "transcript" | "summary";

interface MeetingDetailsSplitViewProps {
  transcript: ReactNode;
  summary: ReactNode;
  activeTab: MeetingDetailsTab;
  onTabChange: (tab: MeetingDetailsTab) => void;
}

export function MeetingDetailsSplitView({
  transcript,
  summary,
  activeTab,
  onTabChange,
}: MeetingDetailsSplitViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [ratio, setRatio] = useState(DEFAULT_RATIO);
  const [isDesktop, setIsDesktop] = useState(true);
  const dragging = useRef(false);

  useEffect(() => {
    setRatio(readStoredRatio());
  }, []);

  useEffect(() => {
    const mediaQuery = window.matchMedia("(min-width: 768px)");
    const updateLayout = () => setIsDesktop(mediaQuery.matches);
    updateLayout();
    mediaQuery.addEventListener("change", updateLayout);

    return () => mediaQuery.removeEventListener("change", updateLayout);
  }, []);

  const onPointerDown = useCallback((event: React.PointerEvent) => {
    event.preventDefault();
    dragging.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
  }, []);

  const onPointerMove = useCallback((event: React.PointerEvent) => {
    if (!dragging.current || !containerRef.current) {
      return;
    }

    const rect = containerRef.current.getBoundingClientRect();
    setRatio(clampRatio((event.clientX - rect.left) / rect.width));
  }, []);

  const onPointerUp = useCallback(() => {
    if (!dragging.current) {
      return;
    }

    dragging.current = false;
    setRatio((current) => {
      writeStoredRatio(current);

      return current;
    });
  }, []);

  const onSeparatorKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      const current = ratio;

      const next =
        event.key === "ArrowLeft"
          ? clampRatio(current - 0.05)
          : event.key === "ArrowRight"
            ? clampRatio(current + 0.05)
            : event.key === "Home"
              ? MIN_RATIO
              : event.key === "End"
                ? MAX_RATIO
                : null;

      if (next === null) {
        return;
      }

      event.preventDefault();
      setRatio(next);
      writeStoredRatio(next);
    },
    [ratio]
  );

  const transcriptPanelProps = isDesktop
    ? { "aria-label": "Transcript", role: "region" as const, tabIndex: -1 }
    : {};

  const summaryPanelProps = isDesktop
    ? { "aria-label": "Summary", role: "region" as const, tabIndex: -1 }
    : {};

  const paneStyle: CSSProperties & { "--transcript-pane-width": string } = {
    "--transcript-pane-width": `${ratio * 100}%`,
  };

  return (
    <Tabs
      value={activeTab}
      onValueChange={(value) =>
        onTabChange(value === "summary" ? "summary" : "transcript")
      }
      className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
    >
      <div className="shrink-0 bg-background px-2 md:hidden">
        <TabsList className="relative h-auto w-full justify-center">
          {TABS.map((tab) => {
            const Icon = tab.icon;

            return (
              <TabsTrigger
                key={tab.value}
                value={tab.value}
                className="relative z-10 flex items-center"
              >
                <Icon className="h-4 w-4" />
                {tab.label}
              </TabsTrigger>
            );
          })}
        </TabsList>
      </div>
      <div
        ref={containerRef}
        className="flex min-h-0 min-w-0 flex-1 flex-col md:flex-row"
        style={paneStyle}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      >
        <TabsContent
          value="transcript"
          forceMount
          className="mt-0 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden md:w-[var(--transcript-pane-width)] md:flex-none md:data-[state=inactive]:flex"
          {...transcriptPanelProps}
        >
          {transcript}
        </TabsContent>
        <div
          role="separator"
          aria-orientation="vertical"
          aria-valuenow={Math.round(ratio * 100)}
          aria-valuemin={Math.round(MIN_RATIO * 100)}
          aria-valuemax={Math.round(MAX_RATIO * 100)}
          aria-valuetext={`Transcript panel ${Math.round(ratio * 100)} percent`}
          aria-label="Resize transcript and summary"
          tabIndex={0}
          className="group relative z-10 hidden w-2 flex-shrink-0 cursor-col-resize items-stretch justify-center focus-visible:ring-2 focus-visible:ring-info focus-visible:outline-none focus-visible:ring-inset md:flex"
          onPointerDown={onPointerDown}
          onKeyDown={onSeparatorKeyDown}
        >
          <div className="h-full w-px bg-muted transition-all duration-150 ease-out group-hover:w-1 group-hover:bg-info group-active:w-1 group-active:bg-info" />
        </div>
        <TabsContent
          value="summary"
          forceMount
          className="mt-0 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden md:data-[state=inactive]:flex"
          {...summaryPanelProps}
        >
          {summary}
        </TabsContent>
      </div>
    </Tabs>
  );
}
