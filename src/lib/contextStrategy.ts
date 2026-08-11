import type { ContextStrategy } from '@/types';

export const COMPRESSION_KEEP_LAST_N_MIN = 0;
export const COMPRESSION_KEEP_LAST_N_MAX = 1000;
export const DEFAULT_COMPRESSION_KEEP_LAST_N = 3;
export const DEFAULT_CONTEXT_STRATEGY: ContextStrategy = 'raw_truncate';

export function isContextStrategy(value: unknown): value is ContextStrategy {
  return value === 'smart_summary' || value === 'raw_truncate' || value === 'raw_strict';
}

export function normalizeContextStrategy(value: unknown): ContextStrategy {
  return isContextStrategy(value) ? value : DEFAULT_CONTEXT_STRATEGY;
}

export function normalizeCompressionKeepLastN(
  value: number | string | null | undefined,
): number {
  if (value == null || value === '') return DEFAULT_COMPRESSION_KEEP_LAST_N;
  const numericValue = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(numericValue)) return DEFAULT_COMPRESSION_KEEP_LAST_N;
  return Math.min(
    COMPRESSION_KEEP_LAST_N_MAX,
    Math.max(COMPRESSION_KEEP_LAST_N_MIN, Math.trunc(numericValue)),
  );
}

export function resolveEffectiveContextStrategy(
  conversation: {
    context_strategy_override?: ContextStrategy | null;
    context_compression?: boolean;
  } | null | undefined,
  globalDefault: ContextStrategy | null | undefined,
): ContextStrategy {
  if (isContextStrategy(conversation?.context_strategy_override)) {
    return conversation.context_strategy_override;
  }
  if (conversation?.context_strategy_override === null) {
    return normalizeContextStrategy(globalDefault);
  }
  // Compatibility for records returned by a backend from before the strategy field existed.
  if (typeof conversation?.context_compression === 'boolean') {
    return conversation.context_compression ? 'smart_summary' : 'raw_truncate';
  }
  return normalizeContextStrategy(globalDefault);
}

export function shouldNotifyContextExclusion(reason: string | null | undefined): boolean {
  return reason === 'input_budget'
    || reason === 'input_budget_exceeded'
    || reason === 'message_limit';
}
