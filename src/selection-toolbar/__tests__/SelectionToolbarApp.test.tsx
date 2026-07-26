import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SelectionToolbarApp } from '../SelectionToolbarApp';

const { executeTool, copyResult, regenerate, stop, nodeRendererProps, storeState } = vi.hoisted(() => ({
  executeTool: vi.fn(async () => {}),
  copyResult: vi.fn(async () => {}),
  regenerate: vi.fn(async () => {}),
  stop: vi.fn(async () => {}),
  nodeRendererProps: vi.fn(),
  storeState: {} as Record<string, unknown>,
}));

const tools = Array.from({ length: 7 }, (_, index) => ({
  id: `tool-${index + 1}`,
  kind: 'ai' as const,
  builtin_key: null,
  name: `Tool ${index + 1}`,
  icon: 'sparkles',
}));

vi.mock('@/stores/selectionToolbarStore', () => ({
  useSelectionToolbarStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector(storeState),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { feature?: string }) =>
      key === 'settings.selectionToolbar.aiFeatureTitle'
        ? `AI ${options?.feature ?? ''}`.trim()
        : key,
  }),
}));

vi.mock('@/i18n', () => ({
  default: {
    changeLanguage: vi.fn(async () => {}),
    dir: vi.fn(() => 'ltr'),
    getFixedT: vi.fn(() => (key: string) => key),
  },
}));

vi.mock('markstream-react', () => ({
  default: (props: { content: string }) => {
    nodeRendererProps(props);
    return <div data-testid="markdown-output">{props.content}</div>;
  },
  enableD2: vi.fn(),
  setCustomComponents: vi.fn(),
  setDefaultI18nMap: vi.fn(),
}));

vi.mock('stream-markdown', () => ({
  registerHighlight: vi.fn(async () => {}),
}));

vi.mock('@/lib/preloadChatRenderers', () => ({
  preloadChatRenderers: vi.fn(async () => {}),
}));

vi.mock('@/stores', () => ({
  useSettingsStore: (selector: (state: Record<string, unknown>) => unknown) => selector({
    settings: {
      code_theme: 'poimandres',
      code_theme_light: 'github-light',
      code_font_family: null,
    },
    ensureSettingsLoaded: vi.fn(async () => {}),
  }),
}));

vi.mock('@/components/chat/chatMarkdownShared', () => ({
  ThinkNode: () => null,
  getChatCodeThemes: () => ({
    darkTheme: 'poimandres',
    lightTheme: 'github-light',
    themes: ['github-light', 'poimandres'],
  }),
  getChatCodeBlockProps: () => ({}),
  CHAT_MERMAID_PROPS: {},
  CHAT_INFOGRAPHIC_PROPS: {},
  CHAT_RENDER_BATCH_PROPS: {},
}));

describe('SelectionToolbarApp', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.assign(storeState, {
      session: {
        selection_id: 'selection',
        tools,
        theme: 'light',
        language: 'en-US',
      },
      run: null,
      surface: 'toolbar',
      copied: false,
      busy: false,
      error: null,
      initialize: vi.fn(async () => {}),
      executeTool,
      stop,
      copyResult,
      regenerate,
      close: vi.fn(async () => {}),
      toggleOverflow: vi.fn(async () => {}),
      dispose: vi.fn(),
    });
  });

  it('shows at most five tools and puts the remainder behind More', () => {
    render(<SelectionToolbarApp />);

    expect(screen.getByRole('button', { name: 'Tool 1' })).toHaveTextContent('Tool 1');
    expect(screen.getByRole('button', { name: 'Tool 5' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Tool 6' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'settings.selectionToolbar.more' })).toBeInTheDocument();
  });

  it('executes the tool on the first primary pointer down', () => {
    render(<SelectionToolbarApp />);
    const button = screen.getByRole('button', { name: 'Tool 1' });

    fireEvent.pointerDown(button, { button: 0 });

    expect(executeTool).toHaveBeenCalledTimes(1);
    expect(executeTool).toHaveBeenCalledWith(tools[0]);
  });

  it('keeps the toolbar above a streaming result titled for the selected AI tool', () => {
    Object.assign(storeState, {
      surface: 'result',
      run: {
        request_id: 'request',
        selection_id: 'selection',
        tool_id: 'tool-1',
        status: 'streaming',
        output: '# Streaming result',
        error: null,
      },
    });

    const { container } = render(<SelectionToolbarApp />);

    expect(screen.getByRole('button', { name: 'Tool 1' })).toBeInTheDocument();
    expect(screen.getByText('AI Tool 1')).toBeInTheDocument();
    expect(container.querySelector('.selection-toolbar__result-stack > .selection-toolbar__bar'))
      .toBeInTheDocument();
    expect(container.querySelector('.aqbot-chat-markdown')).toContainElement(
      screen.getByTestId('markdown-output'),
    );
    expect(nodeRendererProps).toHaveBeenCalledWith(
      expect.objectContaining({
        content: '# Streaming result',
        final: false,
      }),
    );
  });

  it('marks exactly the running tool as selected', () => {
    Object.assign(storeState, {
      surface: 'result',
      run: {
        request_id: 'request',
        selection_id: 'selection',
        tool_id: 'tool-2',
        status: 'streaming',
        output: 'text',
        error: null,
      },
    });

    render(<SelectionToolbarApp />);

    expect(screen.getByRole('button', { name: 'Tool 2' })).toHaveAttribute('data-active', 'true');
    expect(screen.getByRole('button', { name: 'Tool 1' })).not.toHaveAttribute('data-active');
    expect(screen.getByRole('button', { name: 'Tool 3' })).not.toHaveAttribute('data-active');
  });

  it('offers a centered danger stop below the streaming output and keeps regenerate disabled', () => {
    Object.assign(storeState, {
      surface: 'result',
      run: {
        request_id: 'request',
        selection_id: 'selection',
        tool_id: 'tool-1',
        status: 'streaming',
        output: 'partial',
        error: null,
      },
    });

    const { container } = render(<SelectionToolbarApp />);

    const footer = container.querySelector('.selection-toolbar__result-footer');
    expect(footer).toBeInTheDocument();
    const stopButton = screen.getByRole('button', { name: /chat\.stop/ });
    expect(footer).toContainElement(stopButton);
    fireEvent.click(stopButton);
    expect(stop).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'chat.regenerate' })).toBeDisabled();
  });

  it('regenerates from the header once the run has finished and hides the stop footer', () => {
    Object.assign(storeState, {
      surface: 'result',
      run: {
        request_id: 'request',
        selection_id: 'selection',
        tool_id: 'tool-1',
        status: 'completed',
        output: 'done',
        error: null,
      },
    });

    const { container } = render(<SelectionToolbarApp />);

    expect(container.querySelector('.selection-toolbar__result-footer')).not.toBeInTheDocument();
    const regenerateButton = screen.getByRole('button', { name: 'chat.regenerate' });
    expect(regenerateButton).toBeEnabled();
    fireEvent.click(regenerateButton);
    expect(regenerate).toHaveBeenCalledTimes(1);
  });

  it('copies the raw partial output while streaming', () => {
    Object.assign(storeState, {
      surface: 'result',
      run: {
        request_id: 'request',
        selection_id: 'selection',
        tool_id: 'tool-2',
        status: 'streaming',
        output: '**raw** partial',
        error: null,
      },
    });

    render(<SelectionToolbarApp />);
    fireEvent.click(screen.getByRole('button', { name: 'common.copy' }));

    expect(copyResult).toHaveBeenCalledTimes(1);
  });
});
