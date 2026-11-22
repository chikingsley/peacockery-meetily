"use client";

import { usePathname } from "next/navigation";

import Calls from "@/app/calls/page";
import MeetingDetails from "@/app/meeting-details/page";
import Home from "@/app/page";
import Settings from "@/app/settings/page";

export function WorkspacePage({ children }: { children: React.ReactNode }) {
  const path = usePathname();

  if (path === "/meeting-details") {
    return <MeetingDetails />;
  }

  if (path === "/calls") {
    return <Calls />;
  }

  if (path === "/settings") {
    return <Settings />;
  }

  return path === "/" ? <Home /> : children;
}
