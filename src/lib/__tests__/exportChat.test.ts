import { describe, expect, it } from 'vitest';
import {
  renderExportMarkdownHtml,
  buildMarkdownTranscript,
  buildTextTranscript,
  buildJsonTranscript,
  resolveExportSpeakerLabel,
} from '../exportChat';
import type { Message } from '@/types';

function makeMessage(partial: Partial<Message> & Pick<Message, 'id' | 'role' | 'content'>): Message {
  return {
    conversation_id: 'c1',
    status: 'complete',
    created_at: 1_704_067_200_000, // 2024-01-01 UTC-ish; formatTime will map it
    updated_at: 1_704_067_200_000,
    ...partial,
  } as Message;
}

describe('renderExportMarkdownHtml', () => {
  it('renders headings, emphasis, and lists instead of raw markdown source', () => {
    const html = renderExportMarkdownHtml([
      '# Title',
      '',
      'Hello **world** and `code`',
      '',
      '- item one',
      '- item two',
    ].join('\n'));

    expect(html).toContain('<h1');
    expect(html).toContain('Title');
    expect(html).toContain('<strong>world</strong>');
    expect(html).toContain('export-code-inline');
    expect(html).toContain('<ul');
    expect(html).toContain('item one');
    // Should not leave raw markdown markers for these constructs
    expect(html).not.toContain('# Title');
    expect(html).not.toContain('**world**');
  });

  it('renders fenced code blocks with escaped content', () => {
    const html = renderExportMarkdownHtml('```ts\nconst x = 1 < 2\n```');
    expect(html).toContain('export-code-block');
    expect(html).toContain('const x = 1 &lt; 2');
    expect(html).not.toContain('```');
  });

  it('does not pass through raw script/html tags', () => {
    const html = renderExportMarkdownHtml('Hello <script>alert(1)</script> **ok**');
    // Parser strips or escapes untrusted HTML; never emit executable tags.
    expect(html).not.toMatch(/<script\b/i);
    expect(html).not.toContain('onerror=');
    expect(html).toContain('<strong>ok</strong>');
  });

  it('escapes HTML special characters in code spans', () => {
    const html = renderExportMarkdownHtml('use `a < b && c > d`');
    expect(html).toContain('export-code-inline');
    expect(html).toContain('&lt;');
    expect(html).toContain('&amp;&amp;');
    expect(html).toContain('&gt;');
  });

  it('returns empty string for blank input', () => {
    expect(renderExportMarkdownHtml('   ')).toBe('');
  });
});

describe('resolveExportSpeakerLabel', () => {
  it('uses userName for user and model label for assistant', () => {
    const opts = {
      roleLabels: { user: '你', assistant: '助手', system: '系统' },
      userName: '小明',
      getModelLabel: () => 'GPT-4o',
    };
    expect(resolveExportSpeakerLabel(makeMessage({ id: 'u', role: 'user', content: 'hi' }), opts)).toBe('小明');
    expect(resolveExportSpeakerLabel(makeMessage({ id: 'a', role: 'assistant', content: 'yo' }), opts)).toBe('GPT-4o');
    expect(resolveExportSpeakerLabel(makeMessage({ id: 's', role: 'system', content: 'sys' }), opts)).toBe('系统');
  });

  it('falls back to assistant label when model unknown', () => {
    const opts = {
      roleLabels: { user: 'You', assistant: 'Assistant', system: 'System' },
    };
    expect(resolveExportSpeakerLabel(makeMessage({ id: 'a', role: 'assistant', content: 'yo' }), opts)).toBe('Assistant');
  });
});

describe('buildMarkdownTranscript', () => {
  it('optionally strips thinking via includeThinking false', () => {
    const messages = [
      makeMessage({
        id: 'm1',
        role: 'assistant',
        content: '<think data-aqbot="1">secret</think>\n\nVisible answer',
      }),
    ];
    const withThink = buildMarkdownTranscript(messages, 't');
    const withoutThink = buildMarkdownTranscript(messages, 't', { includeThinking: false });
    expect(withThink).toContain('secret');
    expect(withoutThink).not.toContain('secret');
    expect(withoutThink).toContain('Visible answer');
  });

  it('uses i18n speaker labels, model name, and time footer', () => {
    const messages = [
      makeMessage({ id: 'u1', role: 'user', content: '你好', created_at: 1_704_067_200_000 }),
      makeMessage({
        id: 'a1',
        role: 'assistant',
        content: '世界',
        model_id: 'gpt-4o',
        created_at: 1_704_067_260_000,
      }),
    ];
    const md = buildMarkdownTranscript(messages, '会话', {
      roleLabels: { user: '你', assistant: '助手', system: '系统' },
      userName: '小明',
      getModelLabel: (m) => (m.role === 'assistant' ? 'GPT-4o' : undefined),
      formatTime: () => '12:00:00',
    });

    expect(md).toContain('# 会话');
    expect(md).toContain('## 小明');
    expect(md).toContain('## GPT-4o');
    expect(md).not.toContain('## 助手');
    expect(md).not.toContain('## Assistant');
    expect(md).toContain('你好');
    expect(md).toContain('世界');
    // time appears after each message body
    expect(md.match(/12:00:00/g)?.length).toBe(2);
  });
});

describe('buildTextTranscript', () => {
  it('mirrors markdown speaker/time presentation', () => {
    const messages = [
      makeMessage({ id: 'u1', role: 'user', content: 'hi' }),
      makeMessage({ id: 'a1', role: 'assistant', content: 'hey' }),
    ];
    const text = buildTextTranscript(messages, 'Chat', {
      roleLabels: { user: '你', assistant: '助手', system: '系统' },
      userName: 'Alice',
      getModelLabel: () => 'Claude',
      formatTime: () => '09:30:00',
    });

    expect(text).toContain('[Alice]');
    expect(text).toContain('[Claude]');
    expect(text).not.toContain('[助手]');
    expect(text).not.toContain('[Assistant]');
    expect(text).toContain('09:30:00');
  });
});

describe('buildJsonTranscript', () => {
  it('includes display name (model for assistant) and formatted time', () => {
    const messages = [
      makeMessage({ id: 'u1', role: 'user', content: '你好' }),
      makeMessage({ id: 'a1', role: 'assistant', content: '世界', model_id: 'gpt-4o' }),
    ];
    const json = JSON.parse(buildJsonTranscript(messages, '会话', {
      roleLabels: { user: '你', assistant: '助手', system: '系统' },
      userName: '小明',
      getModelLabel: (m) => (m.role === 'assistant' ? 'GPT-4o' : undefined),
      formatTime: () => '12:00:00',
      includeThinking: false,
    }));

    expect(json.title).toBe('会话');
    expect(json.messages).toHaveLength(2);
    expect(json.messages[0]).toMatchObject({
      role: 'user',
      name: '小明',
      content: '你好',
      time: '12:00:00',
    });
    expect(json.messages[1]).toMatchObject({
      role: 'assistant',
      name: 'GPT-4o',
      content: '世界',
      time: '12:00:00',
    });
    expect(json.messages[1].name).not.toBe('助手');
    expect(json.messages[1].thinking).toBeUndefined();
  });
});
