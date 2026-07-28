import type { ReactNode } from 'react';

/**
 * Split `text` by case-insensitive occurrences of `query` and wrap matches in <mark>.
 * Empty query returns plain text.
 */
export function highlightMatch(text: string, query: string): ReactNode {
  const q = query.trim();
  if (!q || !text) return text;

  const lowerText = text.toLocaleLowerCase();
  const lowerQuery = q.toLocaleLowerCase();
  const parts: ReactNode[] = [];
  let start = 0;
  let index = lowerText.indexOf(lowerQuery, start);
  let key = 0;

  while (index !== -1) {
    if (index > start) {
      parts.push(text.slice(start, index));
    }
    parts.push(
      <mark
        key={`m-${key++}`}
        style={{
          background: 'var(--ant-color-primary-bg, rgba(22, 119, 255, 0.18))',
          color: 'inherit',
          padding: 0,
          borderRadius: 2,
        }}
      >
        {text.slice(index, index + q.length)}
      </mark>,
    );
    start = index + q.length;
    index = lowerText.indexOf(lowerQuery, start);
  }

  if (start < text.length) {
    parts.push(text.slice(start));
  }

  return parts.length === 1 ? parts[0] : <>{parts}</>;
}
