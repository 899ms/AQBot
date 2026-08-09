import zhCN from '@/i18n/locales/zh-CN.json';

export function translateZhCN(key: string, options?: Record<string, unknown>): string {
  const value = key.split('.').reduce<unknown>((current, segment) => {
    if (!current || typeof current !== 'object' || Array.isArray(current)) return undefined;
    return (current as Record<string, unknown>)[segment];
  }, zhCN);

  if (typeof value !== 'string') return key;
  return value.replace(/{{\s*([^},\s]+)[^}]*}}/g, (placeholder, name: string) => {
    const replacement = options?.[name];
    return replacement === undefined ? placeholder : String(replacement);
  });
}
