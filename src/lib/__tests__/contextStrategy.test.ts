import { describe, expect, it } from 'vitest';
import {
  COMPRESSION_KEEP_LAST_N_MAX,
  DEFAULT_COMPRESSION_KEEP_LAST_N,
  normalizeCompressionKeepLastN,
  resolveEffectiveContextStrategy,
  shouldNotifyContextExclusion,
} from '../contextStrategy';

describe('contextStrategy', () => {
  it.each([20, 21, 100, 200, 999, 1000])(
    'preserves valid keep-last value %i',
    (value) => {
      expect(normalizeCompressionKeepLastN(value)).toBe(value);
    },
  );

  it('normalizes invalid keep-last values into the supported integer range', () => {
    expect(normalizeCompressionKeepLastN(-1)).toBe(0);
    expect(normalizeCompressionKeepLastN(1001)).toBe(COMPRESSION_KEEP_LAST_N_MAX);
    expect(normalizeCompressionKeepLastN(3.9)).toBe(3);
    expect(normalizeCompressionKeepLastN('not-a-number')).toBe(DEFAULT_COMPRESSION_KEEP_LAST_N);
    expect(normalizeCompressionKeepLastN(null)).toBe(DEFAULT_COMPRESSION_KEEP_LAST_N);
  });

  it('resolves overrides, inheritance, and legacy compression flags', () => {
    expect(resolveEffectiveContextStrategy(
      { context_strategy_override: 'raw_strict', context_compression: false },
      'smart_summary',
    )).toBe('raw_strict');
    expect(resolveEffectiveContextStrategy(
      { context_strategy_override: null, context_compression: true },
      'raw_truncate',
    )).toBe('raw_truncate');
    expect(resolveEffectiveContextStrategy(
      { context_compression: true },
      'raw_truncate',
    )).toBe('smart_summary');
    expect(resolveEffectiveContextStrategy(
      { context_compression: false },
      'smart_summary',
    )).toBe('raw_truncate');
    expect(resolveEffectiveContextStrategy(null, undefined)).toBe('raw_truncate');
  });

  it.each([
    ['input_budget', true],
    ['input_budget_exceeded', true],
    ['message_limit', true],
    ['smart_summary', false],
    ['context_window_unknown', false],
    ['context_budget_unknown', false],
    [null, false],
  ] as const)('decides whether exclusion reason %s needs a toast', (reason, expected) => {
    expect(shouldNotifyContextExclusion(reason)).toBe(expected);
  });
});
