/**
 * Audio file extensions accepted by audio import, with their display names.
 * Keep in sync with AUDIO_EXTENSIONS in src-tauri/src/audio/constants.rs.
 */
const AUDIO_FORMATS = {
  mp4: 'MP4',
  m4a: 'M4A',
  wav: 'WAV',
  mp3: 'MP3',
  flac: 'FLAC',
  ogg: 'OGG',
  webm: 'WebM',
  mov: 'MOV',
} as const;

export const isAudioExtension = (ext: string): boolean => Object.hasOwn(AUDIO_FORMATS, ext);

/** Comma-separated list for UI display, e.g. "MP4, M4A, WAV". */
export function getAudioFormatsDisplayList(): string {
  return Object.values(AUDIO_FORMATS).join(', ');
}
