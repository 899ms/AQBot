import { describe, expect, it, vi, beforeEach } from 'vitest';
import { buildExportOptions, buildExportPngOptions } from '../exportChatPresentation';
import type { Message, ProviderConfig } from '@/types';

vi.mock('@/i18n', () => ({
  default: {
    t: (key: string, opts?: { defaultValue?: string }) => {
      const map: Record<string, string> = {
        'chat.you': '你',
        'chat.assistant': '助手',
        'chat.system': '系统',
      };
      return map[key] ?? opts?.defaultValue ?? key;
    },
  },
}));

const providers = [
  {
    id: 'p1',
    name: 'OpenAI',
    enabled: true,
    models: [
      {
        model_id: 'gpt-4o',
        name: 'GPT-4o',
        enabled: true,
      },
    ],
  },
] as unknown as ProviderConfig[];

const theme = {
  colorPrimary: '#1677ff',
  colorPrimaryBg: '#e6f4ff',
  colorPrimaryBorder: '#91caff',
};

describe('buildExportOptions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('uses i18n role labels and profile name', () => {
    const opts = buildExportOptions({
      userName: '小明',
      theme,
      providers,
    });

    expect(opts.roleLabels).toEqual({
      user: '你',
      assistant: '助手',
      system: '系统',
    });
    expect(opts.userName).toBe('小明');
  });

  it('resolves model labels for assistant titles without provider prefix', () => {
    const opts = buildExportOptions({
      userName: '',
      theme,
      providers,
      conversationModelId: 'gpt-4o',
      conversationProviderId: 'p1',
    });

    const label = opts.getModelLabel?.({
      id: 'm1',
      role: 'assistant',
      model_id: 'gpt-4o',
      provider_id: 'p1',
    } as Message);

    expect(label).toBe('GPT-4o');
  });

  it('keeps buildExportPngOptions as an alias', () => {
    expect(buildExportPngOptions).toBe(buildExportOptions);
  });
});
