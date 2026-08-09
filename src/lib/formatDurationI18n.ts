import type { TFunction } from 'i18next';

/**
 * Human-readable duration with i18n:
 * - ms when < 1s
 * - seconds when < 1m
 * - minutes (+ optional seconds) when < 1h
 * - hours + minutes when ≥ 1h
 */
export function formatDurationI18n(ms: number, t: TFunction): string {
  if (!Number.isFinite(ms) || ms < 0) return '';
  const rounded = Math.max(0, Math.round(ms));

  if (rounded < 1000) {
    return t('common.durationMs', { count: rounded });
  }

  if (rounded < 60_000) {
    const s = rounded / 1000;
    const display = Number.isInteger(s) ? s : Math.round(s * 10) / 10;
    return t('common.durationSec', { count: display });
  }

  if (rounded < 3_600_000) {
    const minutes = Math.floor(rounded / 60_000);
    const seconds = Math.round((rounded % 60_000) / 1000);
    if (seconds > 0) {
      return t('common.durationMinSec', {
        minutes,
        seconds,
      });
    }
    return t('common.durationMin', { count: minutes });
  }

  const hours = Math.floor(rounded / 3_600_000);
  const minutes = Math.floor((rounded % 3_600_000) / 60_000);
  if (minutes > 0) {
    return t('common.durationHourMin', {
      hours,
      minutes,
    });
  }
  return t('common.durationHour', { count: hours });
}

/** Parse ACP message meta_json for duration_ms. */
export function parseAcpDurationMs(metaJson?: string | null): number | null {
  if (!metaJson) return null;
  try {
    const parsed = JSON.parse(metaJson) as { duration_ms?: unknown };
    if (typeof parsed.duration_ms === 'number' && Number.isFinite(parsed.duration_ms)) {
      return Math.max(0, Math.round(parsed.duration_ms));
    }
  } catch {
    // ignore
  }
  return null;
}
