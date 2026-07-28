import { act, cleanup, render, renderHook, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useUpdateChecker } from '../useUpdateChecker';

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  confirm: vi.fn(),
  info: vi.fn(),
  messageSuccess: vi.fn(),
  messageError: vi.fn(),
  registerHighlight: vi.fn(async () => {}),
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mocks.check,
}));

vi.mock('@/lib/invoke', () => ({
  isTauri: () => true,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('stream-markdown', () => ({
  registerHighlight: mocks.registerHighlight,
}));

vi.mock('@/components/chat/chatMarkdownShared', async () => {
  const { createElement } = await import('react');
  return {
    getChatCodeThemes: () => ({
      darkTheme: 'poimandres',
      lightTheme: 'github-light',
      themes: ['github-light', 'poimandres'],
    }),
    ChatMarkdownRenderer: (props: {
      content?: string;
      customId: string;
      final: boolean;
    }) => createElement(
      'div',
      {
        'data-testid': 'chat-markdown-renderer',
        'data-custom-id': props.customId,
        'data-final': String(props.final),
      },
      props.content,
    ),
  };
});

vi.mock('antd', async (importOriginal) => {
  const actual = await importOriginal<typeof import('antd')>();
  return {
    ...actual,
    App: {
      ...actual.App,
      useApp: () => ({
        modal: {
          confirm: mocks.confirm,
          info: mocks.info,
        },
        message: {
          success: mocks.messageSuccess,
          error: mocks.messageError,
        },
      }),
    },
  };
});

async function checkForUpdate(update: { version: string; body?: string | null }) {
  mocks.check.mockResolvedValue(update);
  const { result } = renderHook(() => useUpdateChecker());

  await act(async () => {
    expect(await result.current.checkForUpdate()).toBe(true);
  });

  return mocks.confirm.mock.calls[mocks.confirm.mock.calls.length - 1]?.[0];
}

describe('useUpdateChecker release notes', () => {
  beforeEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('renders update.body through the shared chat markdown renderer', async () => {
    const body = '# Changes\n\n- Markdown list\n\n```ts\nconst ready = true;\n```';
    const confirmOptions = await checkForUpdate({ version: '1.2.3', body });

    render(confirmOptions.content);

    expect(screen.getByText('settings.newVersion: 1.2.3')).toBeInTheDocument();
    const renderer = screen.getByTestId('chat-markdown-renderer');
    expect(renderer.textContent).toBe(body);
    expect(renderer).toHaveAttribute('data-custom-id', 'chat');
    expect(renderer).toHaveAttribute('data-final', 'true');
    expect(renderer.closest('.aqbot-chat-markdown')).toHaveStyle({
      maxHeight: '300px',
      overflow: 'auto',
      marginTop: '8px',
    });
    expect(mocks.registerHighlight).toHaveBeenCalledWith({
      themes: ['github-light', 'poimandres'],
    });
  });

  it('does not render a release notes region when update.body is absent', async () => {
    const confirmOptions = await checkForUpdate({ version: '1.2.4', body: null });

    render(confirmOptions.content);

    expect(screen.getByText('settings.newVersion: 1.2.4')).toBeInTheDocument();
    expect(screen.queryByTestId('chat-markdown-renderer')).not.toBeInTheDocument();
    expect(document.querySelector('.aqbot-chat-markdown')).toBeNull();
  });
});
