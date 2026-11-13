/**
 * Audio file extensions accepted by audio import, with their display names.
 * Keep in sync with AUDIO_EXTENSIONS in src-tauri/src/audio/constants.rs.
 */
const AUDIO_FORMATS = {
  flac: "FLAC",
  m4a: "M4A",
  mov: "MOV",
  mp3: "MP3",
  mp4: "MP4",
  ogg: "OGG",
  wav: "WAV",
  webm: "WebM",
} as const;

export const isAudioExtension = (ext: string): boolean =>
  Object.hasOwn(AUDIO_FORMATS, ext);

/** Comma-separated list for UI display, e.g. "MP4, M4A, WAV". */
export function getAudioFormatsDisplayList(): string {
  return Object.values(AUDIO_FORMATS).join(", ");
}
