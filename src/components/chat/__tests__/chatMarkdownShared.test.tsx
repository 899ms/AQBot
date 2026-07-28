import { cleanup, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CHAT_CUSTOM_HTML_TAGS, type ChatMarkdownNode } from '@/lib/chatMarkdown';
import {
  CHAT_INFOGRAPHIC_PROPS,
  CHAT_MERMAID_PROPS,
  CHAT_RENDER_BATCH_PROPS,
  ChatMarkdownRenderer,
} from '../chatMarkdownShared';

const nodeRendererMock = vi.hoisted(
  () => vi.fn((_props: Record<string, unknown>) => null),
);

vi.mock('markstream-react', () => ({
  default: nodeRendererMock,
}));

describe('ChatMarkdownRenderer', () => {
  beforeEach(() => {
    cleanup();
    nodeRendererMock.mockClear();
  });

  it('forwards chat markdown rendering configuration for content', () => {
    render(
      <ChatMarkdownRenderer
        content={'# Release notes\n\n```ts\nconst ready = true;\n```'}
        isDark
        customId="chat"
        final
        codeBlockThemes={['github-light', 'poimandres']}
        codeBlockLightTheme="github-light"
        codeBlockDarkTheme="poimandres"
        codeFontFamily="JetBrains Mono"
      />,
    );

    const props = nodeRendererMock.mock.calls[nodeRendererMock.mock.calls.length - 1]?.[0];
    expect(props).toMatchObject({
      content: '# Release notes\n\n```ts\nconst ready = true;\n```',
      isDark: true,
      customId: 'chat',
      final: true,
      typewriter: false,
      themes: ['github-light', 'poimandres'],
      codeBlockLightTheme: 'github-light',
      codeBlockDarkTheme: 'poimandres',
      codeBlockMonacoOptions: { fontFamily: 'JetBrains Mono' },
      ...CHAT_RENDER_BATCH_PROPS,
    });
    expect(props?.customHtmlTags).toEqual(CHAT_CUSTOM_HTML_TAGS);
    expect(props?.codeBlockProps).toEqual(expect.objectContaining({
      darkTheme: 'poimandres',
      lightTheme: 'github-light',
      renderHeaderActions: expect.any(Function),
      onPreviewCode: expect.any(Function),
    }));
    expect(props?.mermaidProps).toBe(CHAT_MERMAID_PROPS);
    expect(props?.infographicProps).toBe(CHAT_INFOGRAPHIC_PROPS);
  });

  it('forwards pre-parsed nodes without adding content', () => {
    const nodes = [{ type: 'paragraph', raw: 'Ready' } as ChatMarkdownNode];

    render(
      <ChatMarkdownRenderer
        nodes={nodes}
        isDark={false}
        customId="chat"
        final={false}
        codeBlockThemes={['github-light', 'poimandres']}
        codeBlockLightTheme="github-light"
        codeBlockDarkTheme="poimandres"
      />,
    );

    expect(nodeRendererMock.mock.calls[nodeRendererMock.mock.calls.length - 1]?.[0]).toMatchObject({
      content: undefined,
      nodes,
      isDark: false,
      final: false,
    });
  });
});
