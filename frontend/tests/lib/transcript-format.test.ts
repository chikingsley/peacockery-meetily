import { describe, expect, test } from 'bun:test';
import {
  activeTranscriptId,
  formatRecordingTime,
  formatTranscriptLine,
  formatTranscriptText,
  groupBySpeaker,

  speakersOf,
} from '../../src/lib/transcript-format';

describe('transcript formatting', () => {
  test('formats recording-relative times', () => {
    expect(formatRecordingTime(0)).toBe('[00:00]');
    expect(formatRecordingTime(125.7)).toBe('[02:05]');
    expect(formatRecordingTime(3725)).toBe('[1:02:05]');
    expect(formatRecordingTime(null)).toBe('[--:--]');
  });

  test('includes the speaker when present', () => {
    expect(formatTranscriptLine(5, 'Me', 'Hello')).toBe('[00:05] Me: Hello');
    expect(formatTranscriptLine(5, null, 'Hello')).toBe('[00:05] Hello');
  });

  test('copies a transcript one "[mm:ss] Speaker: text" line per row', () => {
    expect(formatTranscriptText([
      { audio_start_time: 1, speaker: 'Me', text: 'Hi' },
      { audio_start_time: 64, speaker: 'Them', text: 'Hello there' },
      { audio_start_time: null, speaker: null, text: 'Noise' },
    ])).toBe('[00:01] Me: Hi\n[01:04] Them: Hello there\n[--:--] Noise');
  });
});

describe('speaker grouping', () => {
  const row = (id: string, speaker: string | null) => ({ id, speaker });

  test('groups consecutive rows of the same speaker only', () => {
    const groups = groupBySpeaker([row('a', 'Me'), row('b', 'Me'), row('c', 'Them'), row('d', 'Me'), row('e', null), row('f', null)]);
    expect(groups.map(g => [g.key, g.speaker, g.items.map(i => i.id).join('')])).toEqual([
      ['a', 'Me', 'ab'],
      ['c', 'Them', 'c'],
      ['d', 'Me', 'd'],
      ['e', null, 'ef'],
    ]);
  });

  test('lists distinct speakers in order of first appearance', () => {
    expect(speakersOf([row('a', 'Them 2'), row('b', null), row('c', 'Me'), row('d', 'Them 2')])).toEqual(['Them 2', 'Me']);
  });
});

describe('audio helpers', () => {


  test('finds the row under the playhead', () => {
    const rows = [
      { id: 'a', audio_start_time: 0 },
      { id: 'b', audio_start_time: 5 },
      { id: 'c', audio_start_time: 12.5 },
    ];
    expect(activeTranscriptId(rows, null)).toBeNull();
    expect(activeTranscriptId(rows, 0)).toBe('a');
    expect(activeTranscriptId(rows, 4.9)).toBe('a');
    expect(activeTranscriptId(rows, 5)).toBe('b');
    expect(activeTranscriptId(rows, 100)).toBe('c');
    expect(activeTranscriptId([{ id: 'x', audio_start_time: 3 }], 1)).toBeNull();
  });
});
