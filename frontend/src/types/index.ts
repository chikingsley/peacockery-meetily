// Summary and UI types. Contract payload types live in ./voice.

import type { PartialBlock as BlockNoteBlock } from "@blocknote/core";

export type { PartialBlock as BlockNoteBlock } from "@blocknote/core";

export interface SummaryDataResponse {
  markdown?: string;
  summary_json?: BlockNoteBlock[];
  reasoning_stripped?: boolean;
}

export type MeetingSummary = SummaryDataResponse;

export type SummaryProcessStatus =
  | "pending"
  | "processing"
  | "completed"
  | "failed"
  | "cancelled"
  | "error"
  | "idle";

export interface SummaryProcessResponse {
  status: SummaryProcessStatus;
  meetingName: string | null;
  meeting_id: string;
  start: string | null;
  end: string | null;
  data: unknown | null;
  error: string | null;
}
