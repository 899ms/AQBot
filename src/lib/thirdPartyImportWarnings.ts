import type { ThirdPartyImportWarning } from '@/types';

export type ImportWarningTranslate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

/**
 * Localize third-party import warnings by stable backend `code`.
 * Falls back to the backend English `message` when the locale key is missing.
 */
export function getThirdPartyImportWarningMessage(
  warning: ThirdPartyImportWarning,
  t: ImportWarningTranslate,
  namespace: 'cherryImport' | 'kelivoImport' | 'chatgptImport' = 'cherryImport',
): string {
  const key = `settings.${namespace}.warnings.${warning.code}`;
  const params = {
    id: warning.sourceId ?? '',
    name: warning.sourceId ?? '',
    defaultValue: warning.message,
  };
  const translated = t(key, params);
  if (!translated || translated === key) {
    return warning.message;
  }
  return translated;
}
