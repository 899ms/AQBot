import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SelectionToolbarApp } from '../SelectionToolbarApp';

const {
  executeTool,
  copyResult,
  regenerate,
  stop,
  setTranslateLanguages,
  nodeRendererProps,
  storeState,
} = vi.hoisted(() => ({
  executeTool: vi.fn(async () => {}),
  copyResult: vi.fn(async () => {}),
  regenerate: vi.fn(async () => {}),
  stop: vi.fn(async () => {}),
  setTranslateLanguages: vi.fn(async () => {}),
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
        display_mode: 'full',
      },
      run: null,
      surface: 'toolbar',
      overflowDirection: 'below',
      copied: false,
      busy: false,
      error: null,
      translateSource: 'auto',
      translateTarget: null,
      initialize: vi.fn(async () => {}),
      executeTool,
      stop,
      copyResult,
      regenerate,
      setTranslateLanguages,
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

  it('renders compact sessions as an icon-only narrow toolbar with tooltips', () => {
    Object.assign(storeState, {
      session: {
        ...(storeState.session as Record<string, unknown>),
        display_mode: 'compact',
      },
    });

    const { container } = render(<SelectionToolbarApp />);

    const first = screen.getByRole('button', { name: 'Tool 1' });
    expect(first).not.toHaveTextContent('Tool 1');
    expect(first).toHaveAttribute('title', 'Tool 1');
    expect(container.querySelector('.selection-toolbar__bar')).toHaveStyle({ width: '230px' });
  });

  it('renders More as a dropdown anchored to the toolbar', () => {
    Object.assign(storeState, {
      surface: 'overflow',
      overflowDirection: 'above',
    });

    const { container } = render(<SelectionToolbarApp />);

    const dropdown = screen.getByRole('menu', {
      name: 'settings.selectionToolbar.more',
    });
    expect(container.querySelector('.selection-toolbar__overflow')).toContainElement(dropdown);
    expect(container.querySelector('.selection-toolbar__overflow'))
      .toHaveAttribute('data-direction', 'above');
    expect(container.querySelector('.selection-toolbar__bar'))
      .toHaveAttribute('data-dropdown-direction', 'above');
    expect(dropdown).toContainElement(screen.getByRole('button', { name: 'Tool 6' }));
    expect(container.querySelector('.selection-toolbar__result')).not.toBeInTheDocument();
  });

  it('keeps the same toolbar DOM node when More opens so its entrance animation cannot replay', () => {
    const { container, rerender } = render(<SelectionToolbarApp />);
    const initialBar = container.querySelector('.selection-toolbar__bar');

    Object.assign(storeState, {
      surface: 'overflow',
      overflowDirection: 'below',
    });
    rerender(<SelectionToolbarApp />);

    expect(container.querySelector('.selection-toolbar__bar')).toBe(initialBar);
    expect(screen.getByRole('menu', {
      name: 'settings.selectionToolbar.more',
    })).toBeInTheDocument();
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

  it('places an icon-only danger stop beside Close without a result footer', () => {
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

    const actions = container.querySelector('.selection-toolbar__result-actions');
    const stopButton = screen.getByRole('button', { name: 'chat.stop' });
    const closeButton = screen.getByRole('button', { name: 'common.close' });
    expect(actions).toContainElement(stopButton);
    expect(stopButton.nextElementSibling).toBe(closeButton);
    expect(stopButton).toHaveClass('ant-btn-dangerous');
    expect(stopButton).not.toHaveTextContent('chat.stop');
    expect(container.querySelector('.selection-toolbar__result-footer')).not.toBeInTheDocument();
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

  it('shows the translate language bar only for the builtin translate tool', () => {
    const translateTool = {
      id: 'translate',
      kind: 'ai' as const,
      builtin_key: 'translate',
      name: null,
      icon: 'languages',
    };
    Object.assign(storeState, {
      session: {
        selection_id: 'selection',
        tools: [translateTool, ...tools],
        theme: 'light',
        language: 'en-US',
        translate_target_language: 'zh-CN',
      },
      surface: 'result',
      run: {
        request_id: 'request',
        selection_id: 'selection',
        tool_id: 'translate',
        status: 'completed',
        output: '翻译结果',
        error: null,
      },
    });

    const { container } = render(<SelectionToolbarApp />);

    const bar = container.querySelector('.selection-toolbar__translate-bar');
    expect(bar).toBeInTheDocument();
    // Auto-detect source keeps the swap button disabled.
    expect(
      screen.getByRole('button', { name: 'settings.selectionToolbar.translateSwap' }),
    ).toBeDisabled();
  });

  it('swaps source and target languages through the translate bar', () => {
    const translateTool = {
      id: 'translate',
      kind: 'ai' as const,
      builtin_key: 'translate',
      name: null,
      icon: 'languages',
    };
    Object.assign(storeState, {
      session: {
        selection_id: 'selection',
        tools: [translateTool],
        theme: 'light',
        language: 'en-US',
        translate_target_language: 'zh-CN',
      },
      surface: 'result',
      translateSource: 'en',
      translateTarget: null,
      run: {
        request_id: 'request',
        selection_id: 'selection',
        tool_id: 'translate',
        status: 'completed',
        output: '翻译结果',
        error: null,
      },
    });

    render(<SelectionToolbarApp />);
    fireEvent.click(
      screen.getByRole('button', { name: 'settings.selectionToolbar.translateSwap' }),
    );

    // Session target (zh-CN) becomes the source; the old source becomes target.
    expect(setTranslateLanguages).toHaveBeenCalledWith('zh-CN', 'en');
  });

  it('hides the translate language bar for other tools', () => {
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

    expect(container.querySelector('.selection-toolbar__translate-bar')).not.toBeInTheDocument();
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
