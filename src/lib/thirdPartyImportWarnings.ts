import type { ThirdPartyImportWarning } from '@/types';

export type ImportWarningTranslate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

/**
 * Localize third-party import warnings by stable backend `code`.
 * Uses the backend message only for codes not represented by a locale key.
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
  };
  const translated = t(key, params);
  if (!translated || translated === key) {
    return warning.message;
  }
  return translated;
}
