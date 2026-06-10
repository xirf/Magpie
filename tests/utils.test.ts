import { describe, expect, test } from 'bun:test';
import { convertSize, convertEta, formatProgress, escapeMarkdown } from '../src/utils';

describe('Utils', () => {
  test('convertSize formats bytes correctly', () => {
    expect(convertSize(0)).toBe('0 B');
    expect(convertSize(1024)).toBe('1 KB');
    expect(convertSize(1536)).toBe('1.5 KB');
    expect(convertSize(1048576)).toBe('1 MB');
    expect(convertSize(1073741824)).toBe('1 GB');
  });

  test('convertEta formats seconds correctly', () => {
    expect(convertEta(-1)).toBe('∞');
    expect(convertEta(8640000)).toBe('∞');
    expect(convertEta(0)).toBe('00:00:00');
    expect(convertEta(45)).toBe('00:00:45');
    expect(convertEta(3665)).toBe('01:01:05');
    expect(convertEta(90065)).toBe('1 day, 01:01:05');
    expect(convertEta(176465)).toBe('2 days, 01:01:05');
  });

  test('formatProgress creates text bar correctly', () => {
    expect(formatProgress(0, 10)).toBe('  0%|░░░░░░░░░░|\n');
    expect(formatProgress(0.5, 10)).toBe(' 50%|█████░░░░░|\n');
    expect(formatProgress(1, 10)).toBe('100%|██████████|\n');
    expect(formatProgress(1.5, 10)).toBe('100%|██████████|\n');
    expect(formatProgress(-0.5, 10)).toBe('  0%|░░░░░░░░░░|\n');
  });

  test('escapeMarkdown escapes markdown special characters', () => {
    expect(escapeMarkdown('hello-world')).toBe('hello\\-world');
    expect(escapeMarkdown('user_name')).toBe('user\\_name');
    expect(escapeMarkdown('*bold*')).toBe('\\*bold\\*');
    expect(escapeMarkdown('[link]')).toBe('\\[link\\]');
    expect(escapeMarkdown('text.')).toBe('text\\.');
  });
});
