import { describe, expect, it } from 'vitest';
import {
  LONG_PASTE_CHAR_THRESHOLD,
  LONG_PASTE_LINE_THRESHOLD,
  countLines,
  createPastedSnippet,
  formatPasteToken,
  insertPasteTokenAtSelection,
  isLongPastedText,
  mergePastedSnippetsIntoContent,
  removePasteTokens,
  replacePasteTokensWithContent,
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

  it('formats and inserts paste tokens at the caret', () => {
    expect(formatPasteToken(3)).toBe('[[paste:#3]]');
    const inserted = insertPasteTokenAtSelection('hello world', 6, 6, 1);
    expect(inserted.value).toBe('hello [[paste:#1]]world');
    expect(inserted.caret).toBe('hello [[paste:#1]]'.length);
  });

  it('replaces the selection when inserting a paste token', () => {
    const inserted = insertPasteTokenAtSelection('aa bb cc', 3, 5, 2);
    expect(inserted.value).toBe('aa [[paste:#2]] cc');
    expect(inserted.caret).toBe('aa [[paste:#2]]'.length);
  });

  it('expands inline tokens in document order', () => {
    const snippets = [
      createPastedSnippet('first body', 1, () => 's1'),
      createPastedSnippet('second body', 2, () => 's2'),
    ];
    const merged = mergePastedSnippetsIntoContent(
      `Read:\n${formatPasteToken(2)}\nThen:\n${formatPasteToken(1)}\nDone.`,
      snippets,
    );
    const idx2 = merged.indexOf('second body');
    const idx1 = merged.indexOf('first body');
    expect(idx2).toBeGreaterThan(-1);
    expect(idx1).toBeGreaterThan(idx2);
    expect(merged).toContain('Read:');
    expect(merged).toContain('Done.');
    expect(merged).toContain('[Pasted text #2 · 1 lines]');
    expect(merged).toContain('[Pasted text #1 · 1 lines]');
  });

  it('appends orphan snippets that have no token in the text', () => {
    const snippets = [
      createPastedSnippet('inlined', 1, () => 's1'),
      createPastedSnippet('orphan body', 2, () => 's2'),
    ];
    const merged = mergePastedSnippetsIntoContent(
      `Please summarize\n${formatPasteToken(1)}`,
      snippets,
    );
    expect(merged).toContain('Please summarize');
    expect(merged).toContain('inlined');
    expect(merged).toContain('orphan body');
    expect(merged.indexOf('inlined')).toBeLessThan(merged.indexOf('orphan body'));
  });

  it('allows sending only tokens / only orphans without user prose', () => {
    const snippets = [createPastedSnippet('only paste', 1, () => 's1')];
    const withToken = mergePastedSnippetsIntoContent(formatPasteToken(1), snippets);
    expect(withToken).toContain('only paste');
    expect(withToken.startsWith('---')).toBe(true);

    const orphanOnly = mergePastedSnippetsIntoContent('   ', snippets);
    expect(orphanOnly).toContain('only paste');
  });

  it('drops dangling tokens when the snippet was removed', () => {
    const snippets = [createPastedSnippet('kept', 2, () => 's2')];
    const merged = mergePastedSnippetsIntoContent(
      `A ${formatPasteToken(1)} B ${formatPasteToken(2)} C`,
      snippets,
    );
    expect(merged).not.toContain('[[paste:#1]]');
    expect(merged).toContain('kept');
    expect(merged).toContain('A ');
    expect(merged).toContain(' B ');
    expect(merged).toContain(' C');
  });

  it('truncates oversized snippets when merging', () => {
    const huge = 'x'.repeat(100);
    const snippets = [createPastedSnippet(huge, 1, () => 's1')];
    const merged = mergePastedSnippetsIntoContent(`q\n${formatPasteToken(1)}`, snippets, 20);
    expect(merged).toContain('[Pasted text truncated for model context budget.]');
    expect(merged).not.toContain(huge);
  });

  it('removes tokens when a chip is deleted', () => {
    const value = `before ${formatPasteToken(1)} mid ${formatPasteToken(1)} after`;
    expect(removePasteTokens(value, 1)).toBe('before mid after');
  });

  it('replaces tokens with full content on expand', () => {
    const value = `Q:\n${formatPasteToken(1)}\nend`;
    expect(replacePasteTokensWithContent(value, 1, 'FULL')).toBe('Q:\nFULL\nend');
  });

  it('appends content on expand when the token is already gone', () => {
    expect(replacePasteTokensWithContent('question only', 1, 'FULL')).toBe('question only\nFULL');
    expect(replacePasteTokensWithContent('', 1, 'FULL')).toBe('FULL');
  });
});
