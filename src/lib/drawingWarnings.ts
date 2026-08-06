import type { ImageModelWarning } from '@/types';

export type DrawingWarningTranslate = (
  key: string,
  options: Record<string, unknown>,
) => string;

const WARNING_DEFAULTS: Record<string, string> = {
  unknown_image_profile:
    '{{modelId}} has no verified image parameter profile; only conservative text-to-image requests are enabled.',
  using_fallback_profile:
    '{{modelId}} has no verified image parameter profile; using the adapter default parameter preset.',
  legacy_model:
    '{{modelId}} is a legacy image model; use {{replacement}} for new work.',
  retired_model:
    '{{modelId}} is a retired preview model. Compatible proxies can still serve requests.',
  deprecated_model:
    'This image model is deprecated and scheduled to shut down. Compatible endpoints remain available.',
};

function interpolate(
  template: string,
  values: Record<string, string | null | undefined>,
): string {
  return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => {
    const value = values[name];
    return value == null ? '' : String(value);
  });
}

/**
 * Localize backend image-model lifecycle warnings by stable `code`.
 * Falls back to the backend English message when the code is unknown.
 */
export function getDrawingWarningTitle(
  warning: ImageModelWarning,
  modelId: string,
  t: DrawingWarningTranslate,
): string {
  const defaultValue = WARNING_DEFAULTS[warning.code] ?? warning.message;
  const translated = t(`drawing.warning.${warning.code}`, {
    modelId,
    replacement: warning.replacement_model_id ?? '',
    defaultValue,
  });
  // When t() ignores options and returns the key, fall back to a filled template.
  if (!translated || translated === `drawing.warning.${warning.code}`) {
    return interpolate(defaultValue, {
      modelId,
      replacement: warning.replacement_model_id,
    });
  }
  return translated;
}

export function getDrawingWarningDescription(
  warning: ImageModelWarning,
  t: DrawingWarningTranslate,
): string | undefined {
  const parts: string[] = [];
  if (warning.deadline) {
    parts.push(
      t('drawing.warning.deadline', {
        deadline: warning.deadline,
        defaultValue: 'Deadline: {{deadline}}',
      }),
    );
  }
  if (warning.replacement_model_id) {
    parts.push(
      t('drawing.warning.replacement', {
        modelId: warning.replacement_model_id,
        defaultValue: 'Suggested model: {{modelId}}',
      }),
    );
  }
  if (parts.length === 0) return undefined;
  const separator = t('drawing.warning.separator', {
    defaultValue: ' · ',
  });
  return parts.join(separator);
}

/** Soft profile/compat notices shown inline next to the model label (not full-width alerts). */
const COMPATIBILITY_NOTICE_CODES = new Set([
  'using_fallback_profile',
  'unknown_image_profile',
]);

export function isDrawingCompatibilityNotice(warning: ImageModelWarning): boolean {
  return COMPATIBILITY_NOTICE_CODES.has(warning.code);
}

export function splitDrawingWarnings(warnings: ImageModelWarning[] | undefined | null): {
  compatibilityNotices: ImageModelWarning[];
  blockWarnings: ImageModelWarning[];
} {
  const list = warnings ?? [];
  return {
    compatibilityNotices: list.filter(isDrawingCompatibilityNotice),
    blockWarnings: list.filter((warning) => !isDrawingCompatibilityNotice(warning)),
  };
}
