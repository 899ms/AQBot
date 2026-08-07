/**
 * Encode / decode provider.icon values that pack multiple icon kinds into one string.
 *
 * Storage formats:
 * - model/provider lobe icons: `model:OpenAI`, `provider:OpenAI`, or bare `OpenAI`
 * - emoji: `emoji:😀`
 * - url: `url:https://...`
 * - file: `file:images/...` or `file:data:image/png;base64,...`
 */

export type ProviderIconKind = 'model_icon' | 'emoji' | 'url' | 'file';

export interface ParsedProviderIcon {
  type: ProviderIconKind;
  /** Value passed to IconEditor / renderers (without the type prefix). */
  value: string;
}

const CUSTOM_PREFIXES = ['emoji', 'url', 'file'] as const;

/**
 * Parse a stored provider.icon string into type + value for IconEditor.
 */
export function parseProviderIcon(icon: string | null | undefined): ParsedProviderIcon | null {
  if (!icon) return null;
  const sep = icon.indexOf(':');
  if (sep <= 0) {
    // Bare lobe icon id / key
    return { type: 'model_icon', value: icon };
  }
  const prefix = icon.slice(0, sep);
  const rest = icon.slice(sep + 1);
  if ((CUSTOM_PREFIXES as readonly string[]).includes(prefix) && rest.length > 0) {
    return { type: prefix as 'emoji' | 'url' | 'file', value: rest };
  }
  // model:xxx / provider:xxx / unknown:xxx → keep full string for DynamicLobeIcon
  return { type: 'model_icon', value: icon };
}

/**
 * Encode IconEditor onChange result into provider.icon storage string.
 * Returns empty string when cleared.
 */
export function encodeProviderIcon(
  type: string | null,
  value: string | null,
): string {
  if (!type || value == null || value === '') return '';
  if (type === 'model_icon') return value;
  if (type === 'emoji' || type === 'url' || type === 'file') {
    return `${type}:${value}`;
  }
  return '';
}
