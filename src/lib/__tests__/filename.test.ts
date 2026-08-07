import { describe, expect, it } from 'vitest';
import {
  formatExportFilenameTimestamp,
  sanitizeExportFilename,
  sanitizeFilenamePart,
} from '../filename';

describe('sanitizeFilenamePart', () => {
  it('strips Windows-illegal characters', () => {
    expect(sanitizeFilenamePart('关于:测试?文件*"name"|a')).toBe('关于-测试-文件-name-a');
  });

  it('collapses whitespace and dashes', () => {
    expect(sanitizeFilenamePart('  hello   world -- chat  ')).toBe('hello-world-chat');
  });

  it('falls back when empty after sanitize', () => {
    expect(sanitizeFilenamePart(':::')).toBe('aqbot');
    expect(sanitizeFilenamePart('***', 'chat')).toBe('chat');
  });
});

describe('formatExportFilenameTimestamp', () => {
  it('formats local time as YYYY-MM-DD_HHmmss', () => {
    const date = new Date(2026, 2, 27, 14, 30, 52); // local March 27, 2026 14:30:52
    expect(formatExportFilenameTimestamp(date)).toBe('2026-03-27_143052');
  });
});

describe('sanitizeExportFilename', () => {
  it('appends extension, sanitizes title, and suffixes timestamp', () => {
    const at = new Date(2026, 2, 27, 14, 30, 52);
    expect(sanitizeExportFilename('对话: 计划?', 'png', 'chat', at)).toBe(
      '对话-计划 - 2026-03-27_143052.png',
    );
  });

  it('normalizes extension without leading dot', () => {
    const at = new Date(2026, 0, 1, 0, 0, 0);
    expect(sanitizeExportFilename('notes', '.md', 'chat', at)).toBe(
      'notes - 2026-01-01_000000.md',
    );
  });
});
