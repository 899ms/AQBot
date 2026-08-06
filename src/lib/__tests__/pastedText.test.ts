import { describe, expect, it } from 'vitest';
import {
  LONG_PASTE_CHAR_THRESHOLD,
  LONG_PASTE_LINE_THRESHOLD,
  countLines,
  createPastedSnippet,
  isLongPastedText,
  mergePastedSnippetsIntoContent,
} from '../pastedText';

describe('pastedText helpers', () => {
  it('counts lines without inflating trailing newlines', () => {
    expect(countLines('')).toBe(0);
    expect(countLines('hello')).toBe(1);
    expect(countLines('a\nb\nc')).toBe(3);
    expect(countLines('a\nb\nc\n')).toBe(3);
    expect(countLines('a\r\nb\r\nc')).toBe(3);
  });

  it('detects long pastes by character or line threshold', () => {
    expect(isLongPastedText('short')).toBe(false);
    expect(isLongPastedText('x'.repeat(LONG_PASTE_CHAR_THRESHOLD))).toBe(true);
    expect(isLongPastedText(Array.from({ length: LONG_PASTE_LINE_THRESHOLD }, (_, i) => `line ${i}`).join('\n'))).toBe(true);
    expect(isLongPastedText(Array.from({ length: LONG_PASTE_LINE_THRESHOLD - 1 }, (_, i) => `line ${i}`).join('\n'))).toBe(false);
  });

  it('creates snippets with stable metadata', () => {
    const snippet = createPastedSnippet('a\nb\nc', 2, () => 'fixed-id');
    expect(snippet).toEqual({
      id: 'fixed-id',
      content: 'a\nb\nc',
      lineCount: 3,
      index: 2,
    });
  });

  it('merges snippets into content for the model', () => {
    const snippets = [
      createPastedSnippet('hello world', 1, () => 's1'),
      createPastedSnippet('second block', 2, () => 's2'),
    ];
    const merged = mergePastedSnippetsIntoContent('Please summarize', snippets);
    expect(merged).toContain('Please summarize');
    expect(merged).toContain('[Pasted text #1 · 1 lines]');
    expect(merged).toContain('hello world');
    expect(merged).toContain('[Pasted text #2 · 1 lines]');
    expect(merged).toContain('second block');
  });

  it('allows sending snippets without user text', () => {
    const snippets = [createPastedSnippet('only paste', 1, () => 's1')];
    const merged = mergePastedSnippetsIntoContent('   ', snippets);
    expect(merged.startsWith('---')).toBe(true);
    expect(merged).toContain('only paste');
  });

  it('truncates oversized snippets when merging', () => {
    const huge = 'x'.repeat(100);
    const snippets = [createPastedSnippet(huge, 1, () => 's1')];
    const merged = mergePastedSnippetsIntoContent('q', snippets, 20);
    expect(merged).toContain('[Pasted text truncated for model context budget.]');
    expect(merged).not.toContain(huge);
  });
});
