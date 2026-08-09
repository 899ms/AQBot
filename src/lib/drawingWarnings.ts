import type { ImageModelWarning } from '@/types';

export type DrawingWarningTranslate = (
  key: string,
  options: Record<string, unknown>,
) => string;

const WARNING_KEYS: Readonly<Record<string, string>> = {
  unknown_image_profile: 'drawing.warning.unknown_image_profile',
  using_fallback_profile: 'drawing.warning.using_fallback_profile',
  legacy_model: 'drawing.warning.legacy_model',
  retired_model: 'drawing.warning.retired_model',
  deprecated_model: 'drawing.warning.deprecated_model',
};

/**
 * Localize known backend warning codes; preserve unknown backend messages verbatim.
 */
export function getDrawingWarningTitle(
  warning: ImageModelWarning,
  modelId: string,
  t: DrawingWarningTranslate,
): string {
  const key = WARNING_KEYS[warning.code];
  if (!key) return warning.message;
  return t(key, {
    modelId,
    replacement: warning.replacement_model_id ?? '',
  });
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
      }),
    );
  }
  if (warning.replacement_model_id) {
    parts.push(
      t('drawing.warning.replacement', {
        modelId: warning.replacement_model_id,
      }),
    );
  }
  if (parts.length === 0) return undefined;
  const separator = t('drawing.warning.separator', {});
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
