import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { ConversationSearchModal } from '../ConversationSearchModal';

const searchConversations = vi.fn();
const setActiveConversation = vi.fn();

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('antd', async () => {
  const actual = await vi.importActual<typeof import('antd')>('antd');
  return {
    ...actual,
    theme: {
      useToken: () => ({
        token: {
          colorPrimary: '#1677ff',
          colorPrimaryBg: '#e6f4ff',
          colorPrimaryBorder: '#91caff',
          colorBgElevated: '#1f1f1f',
          colorBgMask: 'rgba(0, 0, 0, 0.45)',
          colorText: '#fff',
          colorTextSecondary: '#aaa',
          colorBorderSecondary: '#333',
          colorFillTertiary: '#2a2a2a',
          colorBgContainer: '#1a1a1a',
          boxShadowSecondary: '0 4px 12px rgba(0, 0, 0, 0.15)',
        },
      }),
    },
  };
});

vi.mock('../ConversationIcon', () => ({
  ConversationIcon: ({ conv }: { conv: { id: string; title: string } }) => (
    <span data-testid={`conv-icon-${conv.id}`}>{conv.title[0]}</span>
  ),
}));

vi.mock('@/stores', () => ({
  useConversationStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({
      searchConversations,
      setActiveConversation,
      streamingConversationId: null,
    }),
}));

describe('ConversationSearchModal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    searchConversations.mockResolvedValue([
      {
        conversation: {
          id: 'c1',
          title: 'Hello world chat',
          model_id: 'gpt-4o',
          mode: 'chat',
        },
        matched_message_preview: 'some hello content',
      },
    ]);
  });

  it('does not render when closed', () => {
    const { container } = render(<ConversationSearchModal open={false} onClose={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows loading then results with conversation icons', async () => {
    let resolveSearch!: (value: unknown) => void;
    searchConversations.mockImplementation(
      () => new Promise((resolve) => {
        resolveSearch = resolve;
      }),
    );

    const onClose = vi.fn();
    render(<ConversationSearchModal open onClose={onClose} />);

    const input = screen.getByPlaceholderText('chat.globalSearchPlaceholder');
    fireEvent.change(input, { target: { value: 'hello' } });

    expect(await screen.findByTestId('conversation-search-loading')).toBeInTheDocument();

    await waitFor(() => {
      expect(searchConversations).toHaveBeenCalledWith('hello');
    });

    resolveSearch([
      {
        conversation: {
          id: 'c1',
          title: 'Hello world chat',
          model_id: 'gpt-4o',
          mode: 'chat',
        },
        matched_message_preview: 'some hello content',
      },
    ]);

    expect(await screen.findByTestId('conv-icon-c1')).toBeInTheDocument();

    const item = await screen.findByText((_content, element) => {
      return element?.tagName === 'BUTTON'
        && (element.textContent ?? '').includes('Hello world chat');
    });
    fireEvent.click(item);

    expect(setActiveConversation).toHaveBeenCalledWith('c1');
    expect(onClose).toHaveBeenCalled();
  });

  it('closes on Escape', () => {
    const onClose = vi.fn();
    render(<ConversationSearchModal open onClose={onClose} />);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalled();
  });
});
