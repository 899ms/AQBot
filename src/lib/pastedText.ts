/** Thresholds for collapsing pasted text into a compact snippet chip. */
export const LONG_PASTE_CHAR_THRESHOLD = 2000;
export const LONG_PASTE_LINE_THRESHOLD = 40;

/** Soft cap when merging snippets into the outgoing message content. */
export const PASTED_SNIPPET_CHAR_LIMIT = 96_000;

export type PastedSnippet = {
  id: string;
  content: string;
  lineCount: number;
  /** 1-based display index assigned at creation time. */
  index: number;
};

export function countLines(text: string): number {
  if (!text) return 0;
  // Trailing newline should not inflate the count beyond real lines of content.
  const normalized = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  if (normalized.length === 0) return 0;
  const parts = normalized.split('\n');
  if (parts.length > 1 && parts[parts.length - 1] === '') {
    return parts.length - 1;
  }
  return parts.length;
}

export function isLongPastedText(text: string): boolean {
  if (!text) return false;
  if (text.length >= LONG_PASTE_CHAR_THRESHOLD) return true;
  return countLines(text) >= LONG_PASTE_LINE_THRESHOLD;
}

export function createPastedSnippet(
  content: string,
  nextIndex: number,
  idFactory: () => string = () => `paste-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
): PastedSnippet {
  return {
    id: idFactory(),
    content,
    lineCount: countLines(content),
    index: nextIndex,
  };
}

function truncateToCharLimit(text: string, limit: number): { text: string; truncated: boolean } {
  if (text.length <= limit) return { text, truncated: false };
  // Prefer char-count over code-unit when truncating large pastes.
  let out = '';
  let count = 0;
  for (const ch of text) {
    if (count >= limit) return { text: out, truncated: true };
    out += ch;
    count += 1;
  }
  return { text: out, truncated: false };
}

/**
 * Merge user-typed content with collapsed pasted snippets for the model prompt.
 * Snippets are appended as fenced blocks so the model can tell them apart from the question.
 */
export function mergePastedSnippetsIntoContent(
  userText: string,
  snippets: PastedSnippet[],
  charLimit: number = PASTED_SNIPPET_CHAR_LIMIT,
): string {
  const trimmed = userText.trim();
  if (snippets.length === 0) return trimmed;

  const blocks = snippets.map((snippet) => {
    const { text, truncated } = truncateToCharLimit(snippet.content, charLimit);
    const header = `[Pasted text #${snippet.index} · ${snippet.lineCount} lines]`;
    const body = truncated
      ? `${text}\n\n[Pasted text truncated for model context budget.]`
      : text;
    return `---\n${header}\n${body}\n---`;
  });

  if (!trimmed) return blocks.join('\n\n');
  return `${trimmed}\n\n${blocks.join('\n\n')}`;
}
