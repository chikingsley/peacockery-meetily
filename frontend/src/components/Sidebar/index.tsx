"use client";

import {
  AudioLines,
  CheckCircle2,
  CircleAlert,
  FileText,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  Search,
  Settings2,
  Trash2,
  Upload,
} from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInput,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from "@/components/ui/sidebar";
import { useImportDialog } from "@/contexts/ImportDialogContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";
import { isFinalInProgress } from "@/lib/live-transcript";
import { voiceService, errorMessage } from "@/services/voiceService";
import type { MeetingListItem } from "@/types/voice";

import { useSidebar as useMeetings } from "./SidebarProvider";

function dateGroup(value: string | null) {
  if (!value) {
    return "Earlier";
  }

  const date = new Date(value);
  const today = new Date();

  if (date.toDateString() === today.toDateString()) {
    return "Today";
  }

  today.setDate(today.getDate() - 1);

  if (date.toDateString() === today.toDateString()) {
    return "Yesterday";
  }

  return "Earlier";
}

export default function AppSidebar() {
  const router = useRouter();
  const pathname = usePathname();

  const {
    meetings,
    currentMeeting,
    setCurrentMeeting,
    refetchMeetings,
    searchTranscripts,
    searchResults,
    isSearching,
  } = useMeetings();

  const { isRecording, isStopping, isStartingRecording, meetingId } =
    useRecordingState();

  const { openImportDialog } = useImportDialog();
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<MeetingListItem | null>(null);
  const [deleting, setDeleting] = useState<MeetingListItem | null>(null);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const active = isRecording || isStopping || isStartingRecording;

  useEffect(() => {
    void searchTranscripts(query);
  }, [query, searchTranscripts]);

  const groups = useMemo(() => {
    const term = query.trim().toLowerCase();
    const hits = new Set(searchResults.map((result) => result.id));

    const visible = meetings.filter(
      (meeting) =>
        !term ||
        meeting.title.toLowerCase().includes(term) ||
        hits.has(meeting.id)
    );

    return ["Today", "Yesterday", "Earlier"].map((label) => ({
      label,
      meetings: visible.filter(
        (meeting) => dateGroup(meeting.created_at) === label
      ),
    }));
  }, [meetings, query, searchResults]);

  async function rename() {
    if (!editing || !title.trim()) {
      return;
    }

    setBusy(true);

    try {
      await voiceService.saveMeetingTitle(editing.id, title.trim());

      if (currentMeeting?.id === editing.id) {
        setCurrentMeeting({ id: editing.id, title: title.trim() });
      }

      await refetchMeetings();
      setEditing(null);
      toast.success("Meeting renamed");
    } catch (error) {
      toast.error("Rename failed", { description: errorMessage(error) });
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!deleting) {
      return;
    }

    setBusy(true);

    try {
      await voiceService.deleteMeeting(deleting.id);
      await refetchMeetings();

      if (currentMeeting?.id === deleting.id) {
        router.push("/");
      }

      setDeleting(null);
      toast.success("Meeting deleted");
    } catch (error) {
      toast.error("Delete failed", { description: errorMessage(error) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton size="lg" asChild tooltip="Peacockery Meetily">
                <Link href="/">
                  <Image
                    src="/icon.png"
                    alt=""
                    width={28}
                    height={28}
                    className="size-7 shrink-0 rounded-md"
                  />
                  <span className="truncate font-semibold group-data-[collapsible=icon]:hidden">
                    Meetily
                  </span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton
                variant="primary"
                asChild
                tooltip={active ? "Return to recording" : "New meeting"}
                isActive={pathname === "/"}
              >
                <Link href="/">
                  {active ? <AudioLines /> : <Plus />}
                  <span>{active ? "Return to recording" : "New meeting"}</span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton
                onClick={() => openImportDialog()}
                tooltip="Import audio"
              >
                <Upload />
                <span>Import audio</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
          <div className="relative group-data-[collapsible=icon]:hidden">
            <SidebarInput
              aria-label="Search meetings and transcripts"
              placeholder="Search meetings…"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            {isSearching && (
              <Loader2 className="absolute top-2 right-2 size-4 animate-spin text-muted-foreground" />
            )}
          </div>
        </SidebarHeader>
        <SidebarContent>
          {groups.map(
            (group) =>
              group.meetings.length > 0 && (
                <SidebarGroup key={group.label}>
                  <SidebarGroupLabel>{group.label}</SidebarGroupLabel>
                  <SidebarMenu>
                    {group.meetings.map((meeting) => (
                      <SidebarMenuItem key={meeting.id}>
                        <SidebarMenuButton
                          asChild
                          isActive={
                            pathname === "/meeting-details" &&
                            currentMeeting?.id === meeting.id
                          }
                          tooltip={meeting.title}
                        >
                          <Link
                            href={
                              active && meeting.id === meetingId
                                ? "/"
                                : `/meeting-details?id=${encodeURIComponent(meeting.id)}`
                            }
                            onClick={() => setCurrentMeeting(meeting)}
                          >
                            {isFinalInProgress(meeting.final_status) ? (
                              <Loader2 className="animate-spin" />
                            ) : meeting.final_status === "failed" ? (
                              <CircleAlert />
                            ) : meeting.final_status === "done" ? (
                              <CheckCircle2 />
                            ) : (
                              <FileText />
                            )}
                            <span>{meeting.title}</span>
                          </Link>
                        </SidebarMenuButton>
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <SidebarMenuAction
                              showOnHover
                              aria-label={`Actions for ${meeting.title}`}
                              disabled={
                                busy || (active && meeting.id === meetingId)
                              }
                            >
                              <MoreHorizontal />
                            </SidebarMenuAction>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent side="right" align="start">
                            <DropdownMenuItem
                              onSelect={() => {
                                setEditing(meeting);
                                setTitle(meeting.title);
                              }}
                            >
                              <Pencil />
                              Rename
                            </DropdownMenuItem>
                            <DropdownMenuItem
                              onSelect={() => setDeleting(meeting)}
                            >
                              <Trash2 />
                              Delete meeting
                            </DropdownMenuItem>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </SidebarMenuItem>
                    ))}
                  </SidebarMenu>
                </SidebarGroup>
              )
          )}
          {groups.every((group) => group.meetings.length === 0) && (
            <div className="px-4 py-8 text-center text-sm text-muted-foreground group-data-[collapsible=icon]:hidden">
              <Search className="mx-auto mb-3 size-5" />
              {query
                ? "Try another search."
                : "Your meetings will appear here."}
            </div>
          )}
        </SidebarContent>
        <SidebarFooter>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton
                asChild
                isActive={pathname === "/settings"}
                tooltip="Settings"
              >
                <Link href="/settings">
                  <Settings2 />
                  <span>Settings</span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>
      <Dialog
        open={editing !== null}
        onOpenChange={(open) => {
          if (!open && !busy) {
            setEditing(null);
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Rename meeting</DialogTitle>
            <DialogDescription>
              Choose a title you can find easily later.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void rename();
            }}
            className="space-y-4"
          >
            <Label htmlFor="rename-title">Meeting title</Label>
            <Input
              id="rename-title"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              autoFocus
            />
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setEditing(null)}
                disabled={busy}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={busy || !title.trim()}>
                {busy ? "Saving…" : "Save title"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <AlertDialog
        open={deleting !== null}
        onOpenChange={(open) => {
          if (!open && !busy) {
            setDeleting(null);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this meeting?</AlertDialogTitle>
            <AlertDialogDescription>
              This removes “{deleting?.title}” and its associated meeting data.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy}>Keep meeting</AlertDialogCancel>
            <AlertDialogAction
              disabled={busy}
              onClick={(event) => {
                event.preventDefault();
                void remove();
              }}
            >
              {busy ? "Deleting…" : "Delete meeting"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
