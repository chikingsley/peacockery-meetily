/** Recording-relative time as [MM:SS], or [H:MM:SS] past one hour; [--:--] when unknown. */
export function formatRecordingTime(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return '[--:--]';
  const total = Math.max(0, Math.floor(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = m.toString().padStart(2, '0');
  const ss = s.toString().padStart(2, '0');
  return h > 0 ? `[${h}:${mm}:${ss}]` : `[${mm}:${ss}]`;
}

/** One transcript line for copy/export: `[MM:SS] Speaker: text`. */
export function formatTranscriptLine(start: number | null | undefined, speaker: string | null, text: string): string {
  return speaker
    ? `${formatRecordingTime(start)} ${speaker}: ${text}`
    : `${formatRecordingTime(start)} ${text}`;
}

export interface TranscriptLineLike {
  audio_start_time: number | null;
  speaker: string | null;
  text: string;
}

/** A whole transcript for the clipboard, one `[MM:SS] Speaker: text` line per row. */
export function formatTranscriptText(lines: TranscriptLineLike[]): string {
  return lines.map(l => formatTranscriptLine(l.audio_start_time, l.speaker, l.text)).join('\n');
}

export interface SpeakerGroup<T> {
  /** Key of the first item, stable while rows are appended. */
  key: string;
  speaker: string | null;
  items: T[];
}

/** Consecutive rows of the same speaker, in order. */
export function groupBySpeaker<T extends { id: string; speaker: string | null }>(rows: T[]): SpeakerGroup<T>[] {
  const groups: SpeakerGroup<T>[] = [];
  for (const row of rows) {
    const last = groups[groups.length - 1];
    if (last && last.speaker === row.speaker) last.items.push(row);
    else groups.push({ key: row.id, speaker: row.speaker, items: [row] });
  }
  return groups;
}

/** Distinct speaker labels in order of first appearance. */
export function speakersOf(rows: { speaker: string | null }[]): string[] {
  const seen = new Set<string>();
  for (const row of rows) if (row.speaker) seen.add(row.speaker);
  return [...seen];
}



/** Id of the row under the playhead: the last row (sorted by start) starting at or before `seconds`. */
export function activeTranscriptId(
  rows: { id: string; audio_start_time: number | null }[],
  seconds: number | null,
): string | null {
  if (seconds === null) return null;
  let lo = 0;
  let hi = rows.length - 1;
  let found: string | null = null;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const start = rows[mid].audio_start_time ?? 0;
    if (start <= seconds) {
      found = rows[mid].id;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}
