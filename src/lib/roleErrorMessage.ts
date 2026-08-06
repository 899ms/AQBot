import type { TFunction } from 'i18next';
import { getErrorMessage } from '@/lib/errorMessage';

/** Known backend role validation payloads (after "Validation error: " prefix). */
const ROLE_VALIDATION_KEYS: Record<string, string> = {
  'name cannot be empty': 'roles.validation.nameRequired',
  'system_prompt cannot be empty': 'roles.validation.systemPromptRequired',
};

/**
 * Turn backend / transport errors into user-facing role messages.
 * Maps English validation strings from the Rust layer to i18n keys.
 */
export function getRoleErrorMessage(error: unknown, t: TFunction): string {
  const raw = getErrorMessage(error).trim();
  if (!raw) return t('roles.saveFailed');

  const withoutPrefix = raw.replace(/^Validation error:\s*/i, '').trim();
  const key = ROLE_VALIDATION_KEYS[withoutPrefix.toLowerCase()];
  if (key) return t(key);

  if (/^Validation error:/i.test(raw)) {
    return t('roles.validation.failed', { detail: withoutPrefix || raw });
  }

  if (/^Not found:/i.test(raw)) {
    return t('roles.notFound');
  }

  return raw;
}

export interface RoleDraftValidation {
  name?: string;
  systemPrompt?: string;
}

/** Client-side draft validation (mirrors backend required_text for name / system_prompt). */
export function validateRoleDraft(
  draft: { name: string; systemPrompt: string },
  t: TFunction,
): RoleDraftValidation {
  const errors: RoleDraftValidation = {};
  if (!draft.name.trim()) {
    errors.name = t('roles.validation.nameRequired');
  }
  if (!draft.systemPrompt.trim()) {
    errors.systemPrompt = t('roles.validation.systemPromptRequired');
  }
  return errors;
}
