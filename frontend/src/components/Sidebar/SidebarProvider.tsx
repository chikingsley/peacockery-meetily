// Compatibility exports for existing meeting hooks. Layout lives in ui/sidebar.
export {
  MeetingProvider as SidebarProvider,
  useSidebar,
} from "@/contexts/MeetingContext";

export type { CurrentMeeting, SidebarItem } from "@/contexts/MeetingContext";
