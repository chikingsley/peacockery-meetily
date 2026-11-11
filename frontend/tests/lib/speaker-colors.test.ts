import { describe, expect, test } from 'bun:test';
import { speakerColorClass } from '../../src/lib/speaker-colors';

describe('speaker colours', () => {
  test('Me is blue and nobody else is', () => {
    expect(speakerColorClass('Me')).toContain('blue');
    for (const other of ['Them', 'Them 1', 'Them 2', 'Speaker 1', 'Speaker 7', 'Alice', 'Bob', 'Carol']) {
      expect(speakerColorClass(other)).not.toContain('blue');
    }
  });

  test('uses the configured me label', () => {
    expect(speakerColorClass('Simon', 'Simon')).toContain('blue');
    expect(speakerColorClass('Me', 'Simon')).not.toContain('blue');
  });

  test('is stable per speaker and distinct for numbered speakers', () => {
    expect(speakerColorClass('Them')).toBe(speakerColorClass('Them'));
    expect(speakerColorClass(' Alice ')).toBe(speakerColorClass('Alice'));
    const numbered = [1, 2, 3, 4, 5, 6].map(n => speakerColorClass(`Them ${n}`));
    expect(new Set(numbered).size).toBe(6);
    expect(speakerColorClass('Speaker 1')).toBe(speakerColorClass('Them 1'));
  });
});
