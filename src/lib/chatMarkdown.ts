import { getMarkdown, parseMarkdownToStructure, type BaseNode } from 'stream-markdown-parser';
import { normalizeHtmlRenderContent } from './chatHtmlRender';
import { normalizeThinkTagsForMarkdown } from './thinkTags';

export type ChatMarkdownNode = BaseNode;

export const CHAT_CUSTOM_HTML_TAGS = [
  'think',
  'web-search-query',
  'web-search',
  'knowledge-retrieval',
  'memory-retrieval',
  'tool-call',
  'acp-plan',
  'html-render',
  'img',
] as const;

const AQBOT_DISPLAY_TAGS = new Set([
  'web-search-query',
  'web-search',
  'knowledge-retrieval',
  'memory-retrieval',
  'tool-call',
  'acp-plan',
]);
const LONG_CONTENT_PLAIN_TEXT_THRESHOLD = 100_000;
const UNBALANCED_HTML_TAG_THRESHOLD = 20;

export type StripAqbotTagsOptions = {
  stripThink?: boolean;
};

export type ChatContentPlainTextOptions = {
  role: 'user' | 'assistant' | 'system' | 'tool';
  isStreaming?: boolean;
};

type SafeParseOptions = {
  parse?: (content: string) => ChatMarkdownNode[];
};

function isTagNameChar(ch: string) {
  return /[a-zA-Z0-9_-]/.test(ch);
}

function isWhitespace(ch: string) {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r' || ch === '\f';
}

function findTagEnd(content: string, startIndex: number) {
  let quote: string | null = null;
  for (let index = startIndex; index < content.length; index += 1) {
    const ch = content[index];
    if (quote) {
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === '>') return index;
  }
  return -1;
}

function parseHtmlLikeTag(content: string, startIndex: number) {
  if (content[startIndex] !== '<') return null;
  let index = startIndex + 1;
  let closing = false;
  while (index < content.length && isWhitespace(content[index])) index += 1;
  if (content[index] === '/') {
    closing = true;
    index += 1;
    while (index < content.length && isWhitespace(content[index])) index += 1;
  }

  const nameStart = index;
  while (index < content.length && isTagNameChar(content[index])) index += 1;
  if (index === nameStart) return null;

  const name = content.slice(nameStart, index).toLowerCase();
  const next = content[index] ?? '';
  if (next && !isWhitespace(next) && next !== '/' && next !== '>') return null;

  const end = findTagEnd(content, index);
  if (end === -1) return null;
  const raw = content.slice(startIndex, end + 1);
  return {
    name,
    raw,
    closing,
    selfClosing: /\/\s*>$/.test(raw),
    end,
  };
}

function hasAqbotDataAttr(openTag: string) {
  return /\bdata-aqbot\s*=\s*(?:"1"|'1'|1)(?=\s|\/|>)/i.test(openTag);
}

function findClosingTagEnd(content: string, tagName: string, fromIndex: number) {
  const lower = content.toLowerCase();
  const needle = `</${tagName}`;
  let index = fromIndex;
  for (;;) {
    const closeStart = lower.indexOf(needle, index);
    if (closeStart === -1) return null;
    const next = lower[closeStart + needle.length] ?? '';
    if (next && !isWhitespace(next) && next !== '>') {
      index = closeStart + needle.length;
      continue;
    }
    const closeEnd = findTagEnd(content, closeStart + needle.length);
    if (closeEnd === -1) return null;
    return closeEnd;
  }
}

function consumeTrailingWhitespace(content: string, fromIndex: number) {
  let index = fromIndex;
  while (index < content.length && isWhitespace(content[index])) index += 1;
  return index;
}

function stripCompleteAqbotHtmlTags(content: string, options: StripAqbotTagsOptions) {
  const stripThink = options.stripThink ?? true;
  let output = '';
  let cursor = 0;

  while (cursor < content.length) {
    const openStart = content.indexOf('<', cursor);
    if (openStart === -1) {
      output += content.slice(cursor);
      break;
    }

    const tag = parseHtmlLikeTag(content, openStart);
    if (!tag || tag.closing || tag.selfClosing) {
      output += content.slice(cursor, openStart + 1);
      cursor = openStart + 1;
      continue;
    }

    const shouldStrip = tag.name === 'think'
      ? stripThink
      : AQBOT_DISPLAY_TAGS.has(tag.name) && hasAqbotDataAttr(tag.raw);
    if (!shouldStrip) {
      output += content.slice(cursor, tag.end + 1);
      cursor = tag.end + 1;
      continue;
    }

    const closeEnd = findClosingTagEnd(content, tag.name, tag.end + 1);
    if (closeEnd === null) {
      output += content.slice(cursor, tag.end + 1);
      cursor = tag.end + 1;
      continue;
    }

    output += content.slice(cursor, openStart);
    cursor = consumeTrailingWhitespace(content, closeEnd + 1);
  }

  return output;
}

function findCompleteMcpFenceEnd(content: string, fromIndex: number) {
  let lineStart = fromIndex;
  while (lineStart < content.length) {
    const lineEnd = content.indexOf('\n', lineStart);
    const end = lineEnd === -1 ? content.length : lineEnd;
    if (content.slice(lineStart, end).trim() === ':::') {
      return lineEnd === -1 ? end : lineEnd + 1;
    }
    if (lineEnd === -1) return null;
    lineStart = lineEnd + 1;
  }
  return null;
}

function stripCompleteMcpFences(content: string) {
  let output = '';
  let cursor = 0;

  while (cursor < content.length) {
    const start = content.indexOf(':::mcp ', cursor);
    if (start === -1) {
      output += content.slice(cursor);
      break;
    }
    if (start > 0 && content[start - 1] !== '\n') {
      output += content.slice(cursor, start + 1);
      cursor = start + 1;
      continue;
    }

    const openLineEnd = content.indexOf('\n', start);
    if (openLineEnd === -1) {
      output += content.slice(cursor);
      break;
    }

    const fenceEnd = findCompleteMcpFenceEnd(content, openLineEnd + 1);
    if (fenceEnd === null) {
      output += content.slice(cursor);
      break;
    }

    output += content.slice(cursor, start);
    if (output && !output.endsWith('\n') && fenceEnd < content.length && content[fenceEnd] !== '\n') {
      output += '\n';
    }
    cursor = fenceEnd;
  }

  return output;
}

function countHtmlLikeTagBalance(content: string) {
  let openCount = 0;
  let closeCount = 0;
  let index = 0;

  while (index < content.length) {
    const start = content.indexOf('<', index);
    if (start === -1) break;
    const tag = parseHtmlLikeTag(content, start);
    if (!tag) {
      index = start + 1;
      continue;
    }
    if (tag.closing) closeCount += 1;
    else if (!tag.selfClosing) openCount += 1;
    index = tag.end + 1;
  }

  return { openCount, closeCount };
}

function plainTextNodes(content: string): ChatMarkdownNode[] {
  return [{
    type: 'paragraph',
    children: [{
      type: 'text',
      content,
      raw: content,
    }],
    raw: content,
  } as ChatMarkdownNode];
}

/**
 * Strip all aqbot-injected custom tags (with `data-aqbot="1"` attribute) and
 * MCP tool call fenced blocks (`:::mcp ... :::`) from content.
 * Used when copying message text so display-only tags don't pollute the clipboard.
 */
export function stripAqbotTags(
  content: string,
  options: StripAqbotTagsOptions = {},
): string {
  return stripCompleteMcpFences(stripCompleteAqbotHtmlTags(content, options)).trim();
}

export function shouldUsePlainTextChatContent(
  content: string,
  options: ChatContentPlainTextOptions,
): boolean {
  if (options.role !== 'user' || options.isStreaming || content.length < LONG_CONTENT_PLAIN_TEXT_THRESHOLD) {
    return false;
  }

  const { openCount, closeCount } = countHtmlLikeTagBalance(content);
  return openCount - closeCount >= UNBALANCED_HTML_TAG_THRESHOLD;
}

const chatMarkdown = getMarkdown('aqbot-chat', {
  customHtmlTags: CHAT_CUSTOM_HTML_TAGS,
});

function unwrapStandaloneHtmlRenderNodes(nodes: ChatMarkdownNode[]) {
  return nodes.map((node) => {
    const children = (node as { children?: ChatMarkdownNode[] }).children;
    if (node.type === 'paragraph' && children?.length === 1 && children[0]?.type === 'html-render') {
      return children[0];
    }
    return node;
  });
}

export function parseChatMarkdown(content: string): ChatMarkdownNode[] {
  const nodes = parseMarkdownToStructure(normalizeHtmlRenderContent(normalizeThinkTagsForMarkdown(content), { final: true }), chatMarkdown, {
    customHtmlTags: [...CHAT_CUSTOM_HTML_TAGS],
    final: true,
  });
  return unwrapStandaloneHtmlRenderNodes(nodes);
}

export function safeParseChatMarkdown(
  content: string,
  options: SafeParseOptions = {},
): ChatMarkdownNode[] {
  try {
    return (options.parse ?? parseChatMarkdown)(content);
  } catch (error) {
    console.error('Failed to parse chat markdown:', error);
    return plainTextNodes(content);
  }
}
