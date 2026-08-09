import { App } from 'antd';
import { act, createEvent, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ACP_STATUS_CANCELLING,
  ACP_STATUS_FIRST_OUTPUT_SILENCE,
  useAcpStore,
} from '@/stores/acpStore';
import type { AcpSessionSnapshot } from '@/types/acp';
import { AcpConversationPane, localizeAcpStatus } from '../AcpConversationPane';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

const sessionSnapshot: AcpSessionSnapshot = {
  sessionId: 'acp-session-1',
  modes: null,
  agentCapabilities: { loadSession: true },
  configOptions: [
    {
      id: 'mode',
      name: 'Permission',
      category: 'mode',
      type: 'select',
      currentValue: 'read-only',
      options: [
        { value: 'read-only', name: 'Request approval' },
        { value: 'agent', name: 'Agent' },
        { value: 'agent-full-access', name: 'Full access' },
      ],
    },
    {
      id: 'collaboration_mode',
      name: 'Collaboration mode',
      category: 'mode',
      type: 'select',
      currentValue: 'default',
      options: [
        { value: 'default', name: 'Default' },
        { value: 'plan', name: 'Plan' },
      ],
    },
    {
      id: 'model',
      name: 'Model',
      category: 'model',
      type: 'select',
      currentValue: 'gpt-5.6-codex',
      options: [{ value: 'gpt-5.6-codex', name: 'GPT-5.6 Codex' }],
    },
    {
      id: 'reasoning_effort',
      name: 'Reasoning',
      category: 'thought_level',
      type: 'select',
      currentValue: 'high',
      options: [{ value: 'high', name: 'High' }],
    },
  ],
};

vi.mock('@/lib/invoke', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/lib/invoke')>();
  return {
    ...original,
    invoke: invokeMock,
  };
});

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => ({
    t: (key: string, fallback?: string | { defaultValue?: string }) =>
      typeof fallback === 'string' ? fallback : (fallback?.defaultValue ?? key),
  }),
}));

vi.mock('@/components/chat/chatMarkdownShared', () => {
  // Keep this mock self-contained (no imports of Acp* modules) to avoid circular init hangs.
  function decodeXml(value: string) {
    return value
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"')
      .replace(/&apos;/g, "'")
      .replace(/&amp;/g, '&');
  }
  function MockChatMarkdown({ content }: { content?: string }) {
    const text = content ?? '';
    const planMatch = text.match(/<acp-plan\b([^>]*)>([\s\S]*?)<\/acp-plan>/i);
    if (!planMatch) {
      return <div data-testid="chat-markdown" data-content={text} />;
    }
    const attrsRaw = planMatch[1] ?? '';
    const planId = attrsRaw.match(/\bid="([^"]*)"/i)?.[1] ?? '';
    const status = attrsRaw.match(/\bstatus="([^"]*)"/i)?.[1] ?? 'approved';
    const title = attrsRaw.match(/\btitle="([^"]*)"/i)?.[1] ?? 'Plan review';
    const body = decodeXml(planMatch[2] ?? '').trim() || title;
    // Pending markers are composer-only (match AcpPlanNode).
    if (status === 'pending') {
      const [before, after = ''] = text.split(planMatch[0]);
      return (
        <div data-testid="chat-markdown" data-content={text}>
          {before.trim() ? <span data-testid="markdown-before">{before.trim()}</span> : null}
          {after.trim() ? <span data-testid="markdown-after">{after.trim()}</span> : null}
        </div>
      );
    }
    const [before, after = ''] = text.split(planMatch[0]);
    return (
      <div data-testid="chat-markdown" data-content={text}>
        {before.trim() ? <span data-testid="markdown-before">{before.trim()}</span> : null}
        <div
          className="acp-plan-node"
          data-type="acp-plan"
          data-plan-id={planId}
          data-testid="inline-acp-plan"
          style={{ width: '100%', maxWidth: '100%' }}
        >
          <div data-testid="chat-markdown" data-content={body}>{body}</div>
          <span>{title}</span>
          <span>{status === 'abandoned' ? '已取消' : status === 'approved' ? '已批准执行' : status}</span>
        </div>
        {after.trim() ? <span data-testid="markdown-after">{after.trim()}</span> : null}
      </div>
    );
  }

  return {
    ChatMarkdownRenderer: MockChatMarkdown,
    ThinkNode: () => null,
    getCustomAttr: (
      attrs: Record<string, string> | Array<[string, string]> | null | undefined,
      name: string,
    ) => {
      if (!attrs) return undefined;
      if (Array.isArray(attrs)) {
        for (const entry of attrs) {
          if (Array.isArray(entry) && entry[0] === name) return entry[1];
        }
        return undefined;
      }
      const value = attrs[name];
      return typeof value === 'string' ? value : undefined;
    },
    getChatCodeThemes: () => ({ darkTheme: 'dark', lightTheme: 'light', themes: [] }),
  };
});

vi.mock('@/lib/acpAgentIcon', () => ({
  AcpAgentIcon: () => <span data-testid="acp-agent-icon" />,
}));

vi.mock('@/lib/providerIcons', () => ({
  hasKnownModelIcon: (modelId: string) => /gpt|claude|gemini|grok/i.test(modelId),
  SmartModelIcon: ({ modelId }: { modelId: string }) => (
    <span data-testid="smart-model-icon">{modelId}</span>
  ),
}));

vi.mock('../AcpToolCallNode', () => ({
  AcpToolCallNode: () => null,
}));

beforeAll(() => {
  vi.stubGlobal('IntersectionObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  // jsdom does not implement Element.scrollTo; Bubble.List uses it for stick-to-bottom.
  if (typeof Element !== 'undefined' && !Element.prototype.scrollTo) {
    Element.prototype.scrollTo = function scrollToPolyfill() {};
  }
});

describe('AcpConversationPane', () => {
  it('localizes host-owned status codes without rewriting Agent status text', () => {
    const translate = (key: string) => key;
    const interpolate = (
      _key: string,
      fallback: string,
      values?: Record<string, string | number>,
    ) => Object.entries(values ?? {}).reduce(
      (text, [name, value]) => text.split(`{{${name}}}`).join(String(value)),
      fallback,
    );

    const localizedStatuses = [
      [ACP_STATUS_FIRST_OUTPUT_SILENCE, 'agentPage.interactionSilenceHint'],
      [ACP_STATUS_CANCELLING, 'agentPage.interactionCancelling'],
      ['aqbot:cancel-restarting', 'agentPage.interactionCancelRestarting'],
      ['aqbot:using-shared-agent', 'agentPage.interactionUsingSharedAgent'],
      ['aqbot:launching-agent', 'agentPage.interactionLaunchingAgent'],
      ['aqbot:agent-ready', 'agentPage.interactionAgentReady'],
      ['aqbot:restoring-session', 'agentPage.interactionRestoringSession'],
      ['aqbot:saved-session-expired', 'agentPage.interactionSavedSessionExpired'],
      ['aqbot:creating-session', 'agentPage.interactionCreatingSession'],
      ['aqbot:sending-prompt', 'agentPage.interactionSendingPrompt'],
      ['aqbot:session-expired', 'agentPage.interactionSessionExpired'],
    ] as const;

    for (const [status, expectedKey] of localizedStatuses) {
      expect(localizeAcpStatus(status, translate)).toBe(expectedKey);
    }
    expect(localizeAcpStatus(
      'aqbot:grok-retry:{"attempt":2,"maximum":5,"detail":"timeout"}',
      interpolate,
    )).toBe('网络重试 2/5: timeout');
    expect(localizeAcpStatus(
      'aqbot:grok-retry:{"attempt":2}',
      interpolate,
    )).toBe('网络重试 2');
    expect(localizeAcpStatus('aqbot:grok-retry:{}', interpolate)).toBe('网络重试');
    expect(localizeAcpStatus('aqbot:grok-retry:{bad-json}', interpolate))
      .toBe('aqbot:grok-retry:{bad-json}');
    expect(localizeAcpStatus('Agent-defined retry 2/5: timeout', translate))
      .toBe('Agent-defined retry 2/5: timeout');
    expect(localizeAcpStatus('toString', translate)).toBe('toString');
  });
  afterEach(async () => {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 250));
    });
  });

  beforeEach(() => {
    vi.clearAllMocks();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') {
        return { branch: null, branches: [], isRepo: false };
      }
      if (command === 'acp_prepare_session') return sessionSnapshot;
      if (command === 'acp_cancel') return true;
      if (command === 'acp_set_config_option') return sessionSnapshot;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    useAcpStore.setState({
      config: {
        general: {
          idleTimeoutSecs: 300,
          maxConcurrentProcesses: 2,
          permissionDefault: 'default',
          registryRefresh: 'weekly',
        },
        agents: [
          {
            id: 'codex',
            name: 'Codex',
            enabled: true,
            source: 'builtin',
            command: 'codex',
            args: ['acp'],
            sort: 0,
          },
        ],
      },
      projects: [
        {
          id: 'project-1',
          name: 'AQBot',
          root_path: '/tmp/aqbot',
          kind: 'project',
          sort_order: 0,
          created_at: '2026-08-08T00:00:00Z',
          updated_at: '2026-08-08T00:00:00Z',
        },
      ],
      threads: [
        {
          id: 'thread-1',
          project_id: 'project-1',
          agent_id: 'codex',
          title: 'Running task',
          runtime_status: 'running',
          is_pinned: 0,
          sort_order: 0,
          created_at: '2026-08-08T00:00:00Z',
          updated_at: '2026-08-08T00:00:00Z',
        },
      ],
      messages: [],
      activeProjectId: 'project-1',
      activeThreadId: 'thread-1',
      runningByThread: { 'thread-1': true },
      turnActivityByThread: {},
      statusByThread: { 'thread-1': 'Generating' },
      agentReadinessById: {},
      pendingPermissions: {},
      // Seed session so permission/model controls render without racing prepareSession.
      sessionByThread: { 'thread-1': sessionSnapshot },
      preparingByThread: {},
      cancellingByThread: {},
      planByThread: {},
      spawnModelByThread: {},
      spawnReasoningByThread: {},
    });
  });

  it('keeps the stop control enabled and sends session/cancel while running', async () => {
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const stopIcon = container.querySelector('.lucide-square');
    const stopButton = stopIcon?.closest('button');

    expect(stopButton).toBeInTheDocument();
    expect(stopButton).toBeEnabled();
    fireEvent.click(stopButton!);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_cancel', { threadId: 'thread-1' });
    });
  });

  it('keeps the stop control visible when the persisted assistant row is still streaming', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      messages: [
        {
          id: 'assistant-streaming',
          thread_id: 'thread-1',
          role: 'assistant',
          content: '',
          status: 'streaming',
          attachments: [],
          created_at: '2026-08-08T00:00:01Z',
        },
      ],
    });

    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    await waitFor(() => {
      expect(container.querySelector('.lucide-square')?.closest('button')).toBeEnabled();
      expect(container.querySelector('.lucide-arrow-up')).not.toBeInTheDocument();
    });
  });

  it('labels session setup separately when the Agent process is already ready', async () => {
    const pendingSession = new Promise<AcpSessionSnapshot>(() => undefined);
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      statusByThread: {},
      sessionByThread: {},
      preparingByThread: {},
      agentReadinessById: {
        codex: { status: 'ready', error: null },
      },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_session') return pendingSession;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    expect(await screen.findByText('正在准备对话…')).toBeInTheDocument();
    expect(screen.queryByText('正在连接 Agent…')).not.toBeInTheDocument();
  });

  it('queues permission requests in the composer and restores input after the final decision', async () => {
    useAcpStore.setState({
      messages: [
        {
          id: 'assistant-empty',
          thread_id: 'thread-1',
          role: 'assistant',
          content: '',
          status: 'streaming',
          attachments: [],
          created_at: '2026-08-08T00:00:01Z',
        },
      ],
      runningByThread: { 'thread-1': true },
      statusByThread: { 'thread-1': 'Waiting for Grok Build' },
      pendingPermissions: {},
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_respond_permission') return undefined;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    expect(await screen.findByText('Waiting for Grok Build')).toBeInTheDocument();

    act(() => {
      useAcpStore.setState({
        pendingPermissions: {
          'permission-1': {
            threadId: 'thread-1',
            requestId: 'permission-1',
            toolName: 'write_file',
            input: { path: 'README.md' },
            status: 'pending',
            messageId: 'assistant-empty',
            sequence: 1,
            options: [{ id: 'allow_once', label: 'Allow once', variant: 'primary' }],
          },
          'permission-2': {
            threadId: 'thread-1',
            requestId: 'permission-2',
            toolName: 'delete_file',
            input: { path: 'obsolete.txt' },
            status: 'pending',
            messageId: 'assistant-empty',
            sequence: 2,
            options: [{ id: 'deny', label: 'Deny', variant: 'danger' }],
          },
        },
      });
    });

    const decisionComposer = await screen.findByRole('group', { name: '需要权限' });
    expect(decisionComposer).toHaveAttribute('aria-live', 'polite');
    expect(screen.getByText('write_file')).toBeInTheDocument();
    expect(screen.queryByText('delete_file')).not.toBeInTheDocument();
    expect(screen.getByText('1/2')).toBeInTheDocument();
    expect(screen.queryByPlaceholderText('做点什么…')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '停止' })).toBeEnabled();
    expect(screen.getByRole('button', { name: '下一项' })).toBeEnabled();

    // Can navigate between pending approvals without resolving
    fireEvent.click(screen.getByRole('button', { name: '下一项' }));
    expect(await screen.findByText('delete_file')).toBeInTheDocument();
    expect(screen.getByText('2/2')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '上一项' }));
    expect(await screen.findByText('write_file')).toBeInTheDocument();

    const allowButton = screen.getByRole('button', { name: '允许一次' });
    expect(allowButton).toBeEnabled();
    fireEvent.click(allowButton);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_respond_permission', {
        requestId: 'permission-1',
        optionId: 'allow_once',
        feedback: null,
      });
    });
    expect(await screen.findByText('delete_file')).toBeInTheDocument();
    expect(screen.queryByText('write_file')).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText('做点什么…')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '拒绝' }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_respond_permission', {
        requestId: 'permission-2',
        optionId: 'deny',
        feedback: null,
      });
    });
    await waitFor(() => {
      expect(screen.queryByRole('group', { name: '需要权限' })).not.toBeInTheDocument();
      expect(screen.getByPlaceholderText('做点什么…')).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(screen.getByPlaceholderText('做点什么…')).toHaveFocus();
    });
    expect(useAcpStore.getState().pendingPermissions['permission-1']).toBeUndefined();
  });

  it('temporarily closes a streaming Grok think block so progress is visible immediately', async () => {
    useAcpStore.setState({
      messages: [{
        id: 'assistant-thinking',
        thread_id: 'thread-1',
        role: 'assistant',
        content: '<think>正在分析项目结构',
        status: 'streaming',
        attachments: [],
        created_at: '2026-08-08T00:00:01Z',
      }],
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const markdown = await screen.findByTestId('chat-markdown');
    expect(markdown).toHaveAttribute(
      'data-content',
      '<think>\n正在分析项目结构<!--aqbot-thinking-loading-->\n</think>\n\n',
    );
  });

  it('renders the agent-advertised permission, model, and reasoning controls', async () => {
    useAcpStore.setState({ runningByThread: { 'thread-1': false } });
    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    // Restrictive permission choice is localized in the composer trigger.
    expect(await screen.findByText('每次询问')).toBeInTheDocument();
    // Model and reasoning are separate dropdowns next to send
    expect(screen.getByText('GPT-5.6 Codex')).toBeInTheDocument();
    expect(screen.getByText('High')).toBeInTheDocument();
    expect(screen.getByTestId('smart-model-icon')).toBeInTheDocument();
    // Plan tag only appears while plan mode is active (enabled via shortcut)
    expect(screen.queryByLabelText('计划')).not.toBeInTheDocument();
  });

  it('localizes the selected Codex agent permission as allow edits', async () => {
    const agentModeSnapshot: AcpSessionSnapshot = {
      ...sessionSnapshot,
      configOptions: sessionSnapshot.configOptions.map((option) =>
        option.id === 'mode' ? { ...option, currentValue: 'agent' } : option),
    };
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': agentModeSnapshot },
    });
    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const selectedLabel = await screen.findByText('允许编辑');
    expect(selectedLabel.closest('button')).toBeInTheDocument();
    expect(screen.queryByText(/^Agent$/)).not.toBeInTheDocument();

    fireEvent.click(selectedLabel);
    expect(await screen.findByText('每次询问')).toBeInTheDocument();
    expect(screen.getAllByText('允许编辑')).toHaveLength(2);
    expect(screen.getByText('完全访问')).toBeInTheDocument();
  });

  it('shows model icons in the model dropdown and falls back to agent icon', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: {
        'thread-1': {
          ...sessionSnapshot,
          configOptions: [
            ...sessionSnapshot.configOptions.filter((o) => o.id !== 'model'),
            {
              id: 'model',
              name: 'Model',
              category: 'model',
              type: 'select',
              currentValue: 'gpt-5.6-codex',
              options: [
                {
                  value: 'gpt-5.6-codex',
                  name: 'GPT-5.6 Codex',
                  description: 'Frontier coding model',
                },
                {
                  value: 'custom-unknown-model',
                  name: 'Custom Unknown',
                  description: 'No brand icon',
                },
              ],
            },
          ],
        },
      },
    });
    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    fireEvent.click(await screen.findByText('GPT-5.6 Codex'));
    // Descriptions are intentionally omitted from the compact menu
    expect(screen.queryByText('Frontier coding model')).not.toBeInTheDocument();
    expect(screen.getByText('Custom Unknown')).toBeInTheDocument();
    // Known model uses SmartModelIcon; unknown falls back to agent icon in the list
    expect(screen.getAllByTestId('smart-model-icon').length).toBeGreaterThan(0);
    expect(screen.getAllByTestId('acp-agent-icon').length).toBeGreaterThan(0);
  });

  it('toggles fast mode with a solid/outline icon button', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: {
        'thread-1': {
          ...sessionSnapshot,
          configOptions: [
            ...sessionSnapshot.configOptions,
            {
              id: 'fast',
              name: 'Fast',
              category: 'model_config',
              type: 'select',
              currentValue: 'false',
              options: [],
            },
          ],
        },
      },
    });
    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const fastButton = await screen.findByRole('button', { name: 'Fast' });
    expect(fastButton).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(fastButton);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', {
        threadId: 'thread-1',
        configId: 'fast',
        value: true,
      });
    });
  });

  it('highlights the reasoning control in purple when set to the highest level', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: {
        'thread-1': {
          ...sessionSnapshot,
          configOptions: sessionSnapshot.configOptions.map((option) =>
            option.id === 'reasoning_effort'
              ? {
                  ...option,
                  currentValue: 'xhigh',
                  options: [
                    { value: 'low', name: 'Low' },
                    { value: 'medium', name: 'Medium' },
                    { value: 'high', name: 'High' },
                    { value: 'xhigh', name: 'Xhigh' },
                  ],
                }
              : option,
          ),
        },
      },
    });
    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const label = await screen.findByText('Xhigh');
    const button = label.closest('button');
    expect(button).toBeTruthy();
    expect(button).toHaveStyle({ color: '#7c3aed' });
  });

  it('toggles plan mode with Shift+Tab from the composer', async () => {
    useAcpStore.setState({ runningByThread: { 'thread-1': false } });
    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    expect(await screen.findByText('每次询问')).toBeInTheDocument();
    const input = screen.getByPlaceholderText('做点什么…');
    fireEvent.keyDown(input, { key: 'Tab', shiftKey: true });
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', {
        threadId: 'thread-1',
        configId: 'collaboration_mode',
        value: 'plan',
      });
    });
  });

  it('recognizes Copilot URI plan modes without treating mode as a permission selector', async () => {
    const copilotSnapshot: AcpSessionSnapshot = {
      sessionId: 'copilot-session',
      agentCapabilities: { loadSession: true },
      modes: {
        currentModeId: 'https://agentclientprotocol.com/protocol/session-modes#agent',
        availableModes: [
          {
            id: 'https://agentclientprotocol.com/protocol/session-modes#agent',
            name: 'Agent',
          },
          {
            id: 'https://agentclientprotocol.com/protocol/session-modes#plan',
            name: 'Plan',
          },
        ],
      },
      configOptions: [
        {
          id: 'mode',
          name: 'Mode',
          category: 'mode',
          type: 'select',
          currentValue: 'https://agentclientprotocol.com/protocol/session-modes#agent',
          options: [
            {
              value: 'https://agentclientprotocol.com/protocol/session-modes#agent',
              name: 'Agent',
            },
            {
              value: 'https://agentclientprotocol.com/protocol/session-modes#plan',
              name: 'Plan',
            },
          ],
        },
        {
          id: 'allow_all',
          name: 'Allow All',
          category: 'permissions',
          type: 'select',
          currentValue: 'off',
          options: [
            { value: 'on', name: 'On' },
            { value: 'off', name: 'Off' },
          ],
        },
      ],
    };
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': copilotSnapshot },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_session') return copilotSnapshot;
      if (command === 'acp_set_config_option') return copilotSnapshot;
      if (command === 'acp_set_mode') return copilotSnapshot;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    expect(await screen.findByText('每次询问')).toBeInTheDocument();
    // Plan is enabled via Shift+Tab, not a persistent toggle button
    expect(screen.queryByLabelText('计划')).not.toBeInTheDocument();
    const input = screen.getByPlaceholderText('做点什么…');
    fireEvent.keyDown(input, { key: 'Tab', shiftKey: true });
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_mode', {
        threadId: 'thread-1',
        modeId: 'https://agentclientprotocol.com/protocol/session-modes#plan',
      });
    });
  });

  it('renders Grok permissions through its verified runtime adapter', async () => {
    const grokSnapshot: AcpSessionSnapshot = {
      sessionId: 'grok-session',
      agentCapabilities: { loadSession: true },
      modes: {
        currentModeId: 'default',
        availableModes: [
          { id: 'default', name: 'Agent' },
          { id: 'plan', name: 'Plan' },
        ],
      },
      configOptions: [{
        id: 'aqbot_grok_permission',
        name: 'Permissions',
        category: 'permissions',
        type: 'select',
        currentValue: 'bypassPermissions',
        options: [
          { value: 'default', name: 'Ask' },
          { value: 'auto', name: 'Auto' },
          { value: 'bypassPermissions', name: 'Always Approve' },
        ],
      }],
    };
    const snapshotWithPermission = (value: string): AcpSessionSnapshot => ({
      ...grokSnapshot,
      configOptions: grokSnapshot.configOptions.map((option) => ({
        ...option,
        currentValue: value,
      })),
    });
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': grokSnapshot },
    });
    invokeMock.mockImplementation(async (
      command: string,
      args?: { value?: string },
    ) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_session') return grokSnapshot;
      if (command === 'acp_set_config_option') {
        return snapshotWithPermission(String(args?.value));
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    fireEvent.click(await screen.findByText('完全访问'));
    expect(await screen.findByText('自动审批')).toBeInTheDocument();
    expect(screen.queryByText('允许编辑')).not.toBeInTheDocument();
    expect(screen.queryByText('不询问（拒绝）')).not.toBeInTheDocument();
    const askChoices = await screen.findAllByText('每次询问');
    fireEvent.click(askChoices[askChoices.length - 1]);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', {
        threadId: 'thread-1',
        configId: 'aqbot_grok_permission',
        value: 'default',
      });
    });
  });

  it('renders generic permissions advertised only through session modes', async () => {
    const geminiSnapshot: AcpSessionSnapshot = {
      sessionId: 'gemini-session',
      agentCapabilities: {},
      modes: {
        currentModeId: 'yolo',
        availableModes: [
          { id: 'default', name: 'Default' },
          { id: 'auto_edit', name: 'Auto Edit' },
          { id: 'yolo', name: 'YOLO' },
          { id: 'concise', name: 'Concise' },
          { id: 'verbose', name: 'Verbose' },
          { id: 'plan', name: 'Plan' },
        ],
      },
      configOptions: [],
    };
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': geminiSnapshot },
    });
    invokeMock.mockImplementation(async (
      command: string,
      args?: { modeId?: string },
    ) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_session') return geminiSnapshot;
      if (command === 'acp_set_mode') {
        return {
          ...geminiSnapshot,
          modes: { ...geminiSnapshot.modes!, currentModeId: String(args?.modeId) },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    fireEvent.click(await screen.findByText('完全访问'));
    expect(await screen.findByText('允许编辑')).toBeInTheDocument();
    expect(screen.queryByText('Concise')).not.toBeInTheDocument();
    expect(screen.queryByText('Verbose')).not.toBeInTheDocument();
    const askChoices = await screen.findAllByText('每次询问');
    fireEvent.click(askChoices[askChoices.length - 1]);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_mode', {
        threadId: 'thread-1',
        modeId: 'default',
      });
    });
  });

  it('renders Claude permission approval modes separately from plan mode', async () => {
    const claudeSnapshot: AcpSessionSnapshot = {
      sessionId: 'claude-session',
      agentCapabilities: { loadSession: true },
      modes: {
        currentModeId: 'default',
        availableModes: [
          { id: 'auto', name: 'Auto' },
          { id: 'default', name: 'Manual' },
          { id: 'acceptEdits', name: 'Accept Edits' },
          { id: 'plan', name: 'Plan Mode' },
          { id: 'dontAsk', name: "Don't Ask" },
          { id: 'bypassPermissions', name: 'Bypass Permissions' },
        ],
      },
      configOptions: [
        {
          id: 'mode',
          name: 'Mode',
          description: 'Session permission mode',
          category: 'mode',
          type: 'select',
          currentValue: 'default',
          options: [
            { value: 'auto', name: 'Auto' },
            { value: 'default', name: 'Manual' },
            { value: 'acceptEdits', name: 'Accept Edits' },
            { value: 'plan', name: 'Plan Mode' },
            { value: 'dontAsk', name: "Don't Ask" },
            { value: 'bypassPermissions', name: 'Bypass Permissions' },
          ],
        },
      ],
    };
    useAcpStore.setState({
      config: {
        ...useAcpStore.getState().config!,
        agents: [{
          id: 'claude-acp',
          name: 'Claude Agent',
          enabled: true,
          source: 'builtin',
          command: 'npx',
          args: ['--yes', '@agentclientprotocol/claude-agent-acp@0.65.0'],
          sort: 0,
        }],
      },
      threads: [{
        ...useAcpStore.getState().threads[0],
        agent_id: 'claude-acp',
      }],
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': claudeSnapshot },
    });
    const snapshotWithMode = (modeId: string): AcpSessionSnapshot => ({
      ...claudeSnapshot,
      modes: { ...claudeSnapshot.modes!, currentModeId: modeId },
      configOptions: claudeSnapshot.configOptions.map((option) =>
        option.id === 'mode' ? { ...option, currentValue: modeId } : option),
    });
    invokeMock.mockImplementation(async (
      command: string,
      args?: { modeId?: string; value?: string },
    ) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_session') return claudeSnapshot;
      if (command === 'acp_set_config_option') return snapshotWithMode(String(args?.value));
      if (command === 'acp_set_mode') return snapshotWithMode(String(args?.modeId));
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    fireEvent.click(await screen.findByText('每次询问'));
    expect(await screen.findByText('自动审批')).toBeInTheDocument();
    expect(screen.getByText('允许编辑')).toBeInTheDocument();
    expect(screen.getByText('不询问（拒绝）')).toBeInTheDocument();
    expect(screen.getByText('完全访问')).toBeInTheDocument();
    expect(screen.queryByText('Plan Mode')).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('不询问（拒绝）'));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', {
        threadId: 'thread-1',
        configId: 'mode',
        value: 'dontAsk',
      });
    });
    await waitFor(() => {
      const trigger = screen.getAllByText('不询问（拒绝）')
        .map((node) => node.closest('button'))
        .find((button) => button != null);
      expect(trigger).toBeEnabled();
    });

    fireEvent.keyDown(screen.getByPlaceholderText('做点什么…'), { key: 'Tab', shiftKey: true });
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_mode', {
        threadId: 'thread-1',
        modeId: 'plan',
      });
    });

    const planButton = await screen.findByLabelText('计划');
    const permissionTrigger = screen.getAllByText('不询问（拒绝）')
      .map((node) => node.closest('button'))
      .find((button) => button != null);
    expect(permissionTrigger).toBeDisabled();
    const close =
      planButton.querySelector('[aria-label="关闭"]')
      ?? planButton.querySelector('[aria-label="Close"]')
      ?? planButton.querySelector('[aria-label="close"]');
    expect(close).toBeTruthy();
    fireEvent.click(close!);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_mode', {
        threadId: 'thread-1',
        modeId: 'dontAsk',
      });
    });
  });

  it('shows a closable plan button only while plan mode is active', async () => {
    const planOnSnapshot: AcpSessionSnapshot = {
      ...sessionSnapshot,
      configOptions: sessionSnapshot.configOptions.map((option) =>
        option.id === 'collaboration_mode'
          ? { ...option, currentValue: 'plan' }
          : option,
      ),
    };
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': planOnSnapshot },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_session') return planOnSnapshot;
      if (command === 'acp_set_config_option') {
        return {
          ...planOnSnapshot,
          configOptions: planOnSnapshot.configOptions.map((option) =>
            option.id === 'collaboration_mode'
              ? { ...option, currentValue: 'default' }
              : option,
          ),
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const planButton = await screen.findByLabelText('计划');
    expect(planButton).toBeInTheDocument();
    // Close icon cancels plan mode
    const close =
      planButton.querySelector('[aria-label="关闭"]')
      ?? planButton.querySelector('[aria-label="Close"]')
      ?? planButton.querySelector('[aria-label="close"]');
    expect(close).toBeTruthy();
    fireEvent.click(close!);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', {
        threadId: 'thread-1',
        configId: 'collaboration_mode',
        value: 'default',
      });
    });
  });

  it('normalizes uncategorized model/reasoning options and boolean permissions', async () => {
    const genericSnapshot: AcpSessionSnapshot = {
      sessionId: 'generic-session',
      modes: null,
      agentCapabilities: {},
      configOptions: [
        {
          id: 'auto_approve',
          name: 'Permission approval',
          type: 'boolean',
          currentValue: true,
        },
        {
          id: 'active_model',
          name: 'Model',
          type: 'select',
          currentValue: 'model-a',
          options: [{ value: 'model-a', name: 'Model A' }],
        },
        {
          id: 'effort',
          name: 'Reasoning effort',
          type: 'select',
          currentValue: 'high',
          options: [{ value: 'high', name: 'High' }],
        },
      ],
    };
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': genericSnapshot },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_set_config_option') return genericSnapshot;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    expect(await screen.findByText('完全访问')).toBeInTheDocument();
    expect(screen.getByText('Model A')).toBeInTheDocument();
    expect(screen.getByText('High')).toBeInTheDocument();
    fireEvent.click(screen.getByText('完全访问'));
    fireEvent.click(await screen.findByText('每次询问'));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', {
        threadId: 'thread-1',
        configId: 'auto_approve',
        value: false,
      });
    });
  });

  it('shows live plan progress immediately before the stop control', async () => {
    useAcpStore.setState({
      planByThread: {
        'thread-1': {
          entries: [
            { content: 'Inspect ACP runtime', status: 'completed' },
            { content: 'Wire cancel', status: 'in_progress' },
          ],
          completed: 1,
          total: 2,
        },
      },
    });
    render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const progressButton = screen.getByText('1/2').closest('button');
    const stopButton = screen.getByRole('button', { name: '停止' });
    expect(progressButton).not.toBeNull();
    expect(progressButton?.nextElementSibling).toBe(stopButton);
    await screen.findByText('每次询问');
  });

  it('keeps resolved plan review cards in timeline order after leaving plan mode', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      messages: [
        {
          id: 'user-1',
          thread_id: 'thread-1',
          role: 'user',
          content: 'Please plan the rename',
          status: 'done',
          attachments: [],
          created_at: '2026-08-08T00:00:00Z',
        },
        {
          id: 'assistant-1',
          thread_id: 'thread-1',
          role: 'assistant',
          content: 'Here is the plan.',
          status: 'done',
          attachments: [],
          created_at: '2026-08-08T00:00:01Z',
        },
        {
          id: 'user-2',
          thread_id: 'thread-1',
          role: 'user',
          content: 'Continue',
          status: 'done',
          attachments: [],
          created_at: '2026-08-08T00:00:02Z',
        },
      ],
      planDocumentsByThread: {
        'thread-1': [
          {
            id: 'plan-1',
            threadId: 'thread-1',
            messageId: 'assistant-1',
            content: '## Plan\n1. Inspect\n2. Ship',
            title: '审核计划',
            status: 'approved',
            sequence: 1,
            createdAt: '2026-08-08T00:00:01Z',
          },
        ],
      },
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    expect(await screen.findByText('审核计划')).toBeInTheDocument();
    expect(screen.getByText('已批准执行')).toBeInTheDocument();
    const planMarkdown = screen.getAllByTestId('chat-markdown').find(
      (node) => node.getAttribute('data-content') === '## Plan\n1. Inspect\n2. Ship',
    );
    expect(planMarkdown).toBeTruthy();
    expect(screen.getByRole('button', { name: '带入上下文' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '全屏查看' })).toBeInTheDocument();

    // Plan card is ordered after its assistant message, before later turns.
    const assistantMarkdown = screen.getAllByTestId('chat-markdown').find(
      (node) => node.getAttribute('data-content') === 'Here is the plan.',
    );
    const laterUser = screen.getByText('Continue');
    expect(assistantMarkdown).toBeTruthy();
    expect(
      assistantMarkdown!.compareDocumentPosition(planMarkdown!)
        & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      planMarkdown!.compareDocumentPosition(laterUser)
        & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it('renders inline plan markers chronologically inside the assistant message', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      messages: [
        {
          id: 'assistant-1',
          thread_id: 'thread-1',
          role: 'assistant',
          content: [
            'Before plan.',
            '<acp-plan data-aqbot="1" id="plan-1" message="assistant-1" status="abandoned" title="Plan review">## Plan\n1. Inspect\n2. Ship</acp-plan>',
            'After plan was abandoned.',
          ].join('\n\n'),
          status: 'done',
          attachments: [],
          created_at: '2026-08-08T00:00:01Z',
        },
      ],
      planDocumentsByThread: {
        'thread-1': [
          {
            id: 'plan-1',
            threadId: 'thread-1',
            messageId: 'assistant-1',
            content: '## Plan\n1. Inspect\n2. Ship',
            title: 'Plan review',
            status: 'abandoned',
            sequence: 1,
            createdAt: '2026-08-08T00:00:01Z',
          },
        ],
      },
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const inlinePlan = await screen.findByTestId('inline-acp-plan');
    expect(inlinePlan).toHaveAttribute('data-plan-id', 'plan-1');
    expect(inlinePlan).toHaveStyle({ width: '100%', maxWidth: '100%' });
    const before = screen.getByTestId('markdown-before');
    const after = screen.getByTestId('markdown-after');
    expect(before).toHaveTextContent('Before plan.');
    expect(after).toHaveTextContent('After plan was abandoned.');
    // Chronological: before → plan → after inside the same message body.
    expect(
      before.compareDocumentPosition(inlinePlan) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      inlinePlan.compareDocumentPosition(after) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // Marker-backed plans must not also appear as a separate timeline bubble.
    expect(screen.getAllByTestId('inline-acp-plan')).toHaveLength(1);
  });

  it('still renders an inline plan from the marker alone after store wipe (reload)', async () => {
    // Simulates page refresh: messages come back from DB with the full marker
    // body, but planDocumentsByThread is empty until/while hydration runs.
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      messages: [
        {
          id: 'assistant-1',
          thread_id: 'thread-1',
          role: 'assistant',
          content: [
            'Before.',
            '<acp-plan data-aqbot="1" id="plan-persist" message="assistant-1" status="approved" title="Plan review">## Persisted plan body</acp-plan>',
            'After.',
          ].join('\n\n'),
          status: 'done',
          attachments: [],
          created_at: '2026-08-08T00:00:01Z',
        },
      ],
      planDocumentsByThread: {},
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    const inlinePlan = await screen.findByTestId('inline-acp-plan');
    expect(inlinePlan).toHaveAttribute('data-plan-id', 'plan-persist');
    expect(screen.getByText('## Persisted plan body')).toBeInTheDocument();
    expect(screen.getByText('已批准执行')).toBeInTheDocument();
  });

  it('sends a selected image attachment when the agent advertises image capability', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: {
        'thread-1': {
          ...sessionSnapshot,
          agentCapabilities: {
            ...sessionSnapshot.agentCapabilities,
            promptCapabilities: { image: true },
          },
        },
      },
    });
    invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'user-image',
            thread_id: 'thread-1',
            role: 'user',
            content: '附件',
            status: 'done',
            attachments: [{
              id: 'stored-image',
              file_type: 'image/png',
              file_name: 'diagram.png',
              file_path: 'images/stored_diagram.png',
              file_size: 5,
            }],
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'assistant-image',
            thread_id: 'thread-1',
            role: 'assistant',
            content: '',
            status: 'streaming',
            attachments: [],
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command} ${JSON.stringify(args)}`);
    });

    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]');
    const image = new File(['image'], 'diagram.png', { type: 'image/png' });

    fireEvent.change(fileInput!, { target: { files: [image] } });
    expect(await screen.findByText('diagram.png')).toBeInTheDocument();
    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prompt', expect.objectContaining({
        threadId: 'thread-1',
        prompt: '(attachment)',
        attachments: [{
          file_name: 'diagram.png',
          file_type: 'image/png',
          file_size: 5,
          data: 'aW1hZ2U=',
        }],
      }));
    });
  });

  it('rejects images without capability but still sends an arbitrary ordinary file', async () => {
    useAcpStore.setState({ runningByThread: { 'thread-1': false } });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'user-archive',
            thread_id: 'thread-1',
            role: 'user',
            content: '附件',
            status: 'done',
            attachments: [{
              id: 'stored-archive',
              file_type: 'application/zip',
              file_name: 'sources.zip',
              file_path: 'files/stored_sources.zip',
              file_size: 3,
            }],
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'assistant-archive',
            thread_id: 'thread-1',
            role: 'assistant',
            content: '',
            status: 'streaming',
            attachments: [],
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]');

    fireEvent.change(fileInput!, {
      target: { files: [new File(['image'], 'blocked.png', { type: 'image/png' })] },
    });
    expect(await screen.findByText('agentPage.imageAttachmentUnsupported')).toBeInTheDocument();
    expect(screen.queryByText('blocked.png')).not.toBeInTheDocument();

    fireEvent.change(fileInput!, {
      target: { files: [new File(['zip'], 'sources.zip', { type: 'application/zip' })] },
    });
    expect(await screen.findByText('sources.zip')).toBeInTheDocument();
    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prompt', expect.objectContaining({
        threadId: 'thread-1',
        prompt: '(attachment)',
        attachments: [{
          file_name: 'sources.zip',
          file_type: 'application/zip',
          file_size: 3,
          data: 'emlw',
        }],
      }));
    });
  });

  it('removes a pending image when the session loses image capability', async () => {
    const imageSnapshot: AcpSessionSnapshot = {
      ...sessionSnapshot,
      agentCapabilities: {
        ...sessionSnapshot.agentCapabilities,
        promptCapabilities: { image: true },
      },
    };
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      sessionByThread: { 'thread-1': imageSnapshot },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]');
    fireEvent.change(fileInput!, {
      target: { files: [new File(['image'], 'pending.png', { type: 'image/png' })] },
    });
    expect(await screen.findByText('pending.png')).toBeInTheDocument();

    act(() => {
      useAcpStore.setState({
        sessionByThread: {
          'thread-1': {
            ...imageSnapshot,
            agentCapabilities: {
              ...imageSnapshot.agentCapabilities,
              promptCapabilities: { image: false },
            },
          },
        },
      });
    });

    await waitFor(() => expect(screen.queryByText('pending.png')).not.toBeInTheDocument());
    expect(await screen.findByText('agentPage.imageAttachmentUnsupported')).toBeInTheDocument();
  });

  it('collapses a long paste and expands it into the ACP prompt', async () => {
    useAcpStore.setState({ runningByThread: { 'thread-1': false } });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'user-paste',
            thread_id: 'thread-1',
            role: 'user',
            content: 'expanded',
            status: 'done',
            attachments: [],
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'assistant-paste',
            thread_id: 'thread-1',
            role: 'assistant',
            content: '',
            status: 'streaming',
            attachments: [],
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const textarea = screen.getByPlaceholderText('做点什么…') as HTMLTextAreaElement;
    const longText = 'long ACP context '.repeat(180);
    fireEvent(textarea, createEvent.paste(textarea, {
      clipboardData: {
        items: [],
        getData: (type: string) => (type === 'text/plain' ? longText : ''),
      },
    }));

    expect(textarea).toHaveValue('[[paste:#1]]');
    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prompt', expect.objectContaining({
        threadId: 'thread-1',
        prompt: expect.stringContaining(longText),
      }));
    });
  });

  it('restores text and attachments when ACP prompt submission fails', async () => {
    useAcpStore.setState({ runningByThread: { 'thread-1': false } });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prompt') throw new Error('schedule failed');
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]');
    const textarea = screen.getByPlaceholderText('做点什么…') as HTMLTextAreaElement;
    fireEvent.change(fileInput!, {
      target: { files: [new File(['data'], 'retry.bin', { type: 'application/octet-stream' })] },
    });
    fireEvent.change(textarea, { target: { value: 'retry this turn' } });
    expect(await screen.findByText('retry.bin')).toBeInTheDocument();
    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);

    await waitFor(() => {
      expect(textarea).toHaveValue('retry this turn');
      expect(screen.getByText('retry.bin')).toBeInTheDocument();
    });
  });

  it('detaches the submitted attachment before reading and preserves the next draft', async () => {
    let pendingReader: FileReader | undefined;
    const readSpy = vi
      .spyOn(FileReader.prototype, 'readAsDataURL')
      .mockImplementation(function captureReader(this: FileReader) {
        pendingReader = this;
      });
    useAcpStore.setState({ runningByThread: { 'thread-1': false } });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'user-detached',
            thread_id: 'thread-1',
            role: 'user',
            content: '(attachment)',
            status: 'done',
            attachments: [],
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'assistant-detached',
            thread_id: 'thread-1',
            role: 'assistant',
            content: '',
            status: 'streaming',
            attachments: [],
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]')!;
    fireEvent.change(fileInput, {
      target: { files: [new File(['A'], 'submitted.bin')] },
    });
    expect(await screen.findByText('submitted.bin')).toBeInTheDocument();

    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);
    await waitFor(() => expect(screen.queryByText('submitted.bin')).not.toBeInTheDocument());
    expect(pendingReader).toBeDefined();

    fireEvent.change(fileInput, {
      target: { files: [new File(['B'], 'next.bin')] },
    });
    expect(await screen.findByText('next.bin')).toBeInTheDocument();
    Object.defineProperty(pendingReader!, 'result', {
      configurable: true,
      value: 'data:application/octet-stream;base64,QQ==',
    });
    await act(async () => {
      pendingReader!.onload?.(
        new ProgressEvent('load') as unknown as ProgressEvent<FileReader>,
      );
    });

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      'acp_prompt',
      expect.objectContaining({
        attachments: [expect.objectContaining({ file_name: 'submitted.bin' })],
      }),
    ));
    expect(screen.getByText('next.bin')).toBeInTheDocument();
    readSpy.mockRestore();
  });

  it('restores an attachment-only first turn after draft adoption fails to schedule', async () => {
    const createdThread = {
      id: 'thread-created',
      project_id: 'project-1',
      agent_id: 'codex',
      title: 'first.bin',
      runtime_status: 'idle',
      is_pinned: 0,
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    };
    useAcpStore.setState({
      threads: [],
      allThreads: [],
      messages: [],
      activeThreadId: null,
      runningByThread: {},
      sessionByThread: { 'draft:project-1:codex': sessionSnapshot },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_draft') return sessionSnapshot;
      if (command === 'acp_create_thread') return createdThread;
      if (command === 'acp_list_threads' || command === 'acp_list_all_threads') {
        return [createdThread];
      }
      if (command === 'acp_prompt') throw new Error('first turn schedule failed');
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]');
    fireEvent.change(fileInput!, {
      target: { files: [new File(['first'], 'first.bin')] },
    });
    expect(await screen.findByText('first.bin')).toBeInTheDocument();

    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);

    await waitFor(() => {
      expect(useAcpStore.getState().activeThreadId).toBe('thread-created');
      expect(screen.getByText('first.bin')).toBeInTheDocument();
      expect(invokeMock).toHaveBeenCalledWith('acp_create_thread', expect.objectContaining({
        title: 'first.bin',
      }));
    });
  });

  it('does not prepare an already warm draft again before the first send', async () => {
    const createdThread = {
      id: 'thread-warm-draft',
      project_id: 'project-1',
      agent_id: 'codex',
      title: 'hello',
      runtime_status: 'idle',
      is_pinned: 0,
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    };
    useAcpStore.setState({
      threads: [],
      allThreads: [],
      messages: [],
      activeThreadId: null,
      runningByThread: {},
      sessionByThread: { 'draft:project-1:codex': sessionSnapshot },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'acp_prepare_draft') {
        throw new Error('warm draft must not be prepared twice');
      }
      if (command === 'acp_create_thread') return createdThread;
      if (command === 'acp_list_threads' || command === 'acp_list_all_threads') {
        return [createdThread];
      }
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'user-warm-draft',
            thread_id: createdThread.id,
            role: 'user',
            content: 'hello',
            status: 'done',
            attachments: [],
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'assistant-warm-draft',
            thread_id: createdThread.id,
            role: 'assistant',
            content: '',
            status: 'streaming',
            attachments: [],
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    fireEvent.change(screen.getByPlaceholderText('做点什么…'), {
      target: { value: 'hello' },
    });
    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'acp_prompt',
        expect.objectContaining({ threadId: createdThread.id, prompt: 'hello' }),
      );
    });
    expect(invokeMock).not.toHaveBeenCalledWith('acp_prepare_draft', expect.anything());
  });

  it('creates a recent workspace when sending without a selected project', async () => {
    const recentProject = {
      id: 'recent-project',
      name: 'hello anywhere',
      root_path: '/tmp/aqbot-workspace/recent-project',
      kind: 'recent' as const,
      sort_order: 1,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    };
    const recentThread = {
      id: 'recent-thread',
      project_id: recentProject.id,
      agent_id: 'codex',
      title: 'hello anywhere',
      runtime_status: 'idle',
      is_pinned: 0,
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    };
    useAcpStore.setState({
      projects: [],
      threads: [],
      allThreads: [],
      messages: [],
      activeProjectId: null,
      activeThreadId: null,
      runningByThread: {},
      sessionByThread: {},
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_create_recent_thread') {
        return { project: recentProject, thread: recentThread };
      }
      if (command === 'acp_prepare_session') return sessionSnapshot;
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'recent-user',
            thread_id: recentThread.id,
            role: 'user',
            content: 'hello anywhere',
            status: 'done',
            attachments: [],
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'recent-assistant',
            thread_id: recentThread.id,
            role: 'assistant',
            content: '',
            status: 'streaming',
            attachments: [],
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    fireEvent.change(screen.getByPlaceholderText('做点什么…'), {
      target: { value: 'hello anywhere' },
    });
    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      'acp_create_recent_thread',
      { agentId: 'codex', title: 'hello anywhere' },
    ));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      'acp_prompt',
      expect.objectContaining({ threadId: recentThread.id, prompt: 'hello anywhere' }),
    ));
    expect(useAcpStore.getState().activeProjectId).toBe(recentProject.id);
    expect(useAcpStore.getState().activeThreadId).toBe(recentThread.id);
  });

  it('does not restore a failed turn into a different thread composer', async () => {
    let rejectPrompt!: (reason: Error) => void;
    const otherThread = {
      ...useAcpStore.getState().threads[0],
      id: 'thread-2',
      title: 'Other task',
      runtime_status: 'idle',
    };
    useAcpStore.setState((state) => ({
      threads: [...state.threads, otherThread],
      allThreads: [...state.threads, otherThread],
      runningByThread: { 'thread-1': false, 'thread-2': false },
      sessionByThread: {
        'thread-1': sessionSnapshot,
        'thread-2': sessionSnapshot,
      },
    }));
    invokeMock.mockImplementation((command: string) => {
      if (command === 'acp_git_info') {
        return Promise.resolve({ branch: null, branches: [], isRepo: false });
      }
      if (command === 'acp_prompt') {
        return new Promise((_, reject) => {
          rejectPrompt = reject;
        });
      }
      return Promise.reject(new Error(`Unexpected invoke: ${command}`));
    });
    const { container } = render(
      <App>
        <AcpConversationPane />
      </App>,
    );
    const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]');
    const textarea = screen.getByPlaceholderText('做点什么…') as HTMLTextAreaElement;
    fireEvent.change(fileInput!, {
      target: { files: [new File(['old'], 'old-scope.bin')] },
    });
    fireEvent.change(textarea, { target: { value: 'old scope text' } });
    fireEvent.click(container.querySelector('.lucide-arrow-up')!.closest('button')!);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      'acp_prompt',
      expect.objectContaining({ threadId: 'thread-1' }),
    ));

    act(() => {
      useAcpStore.setState({ activeThreadId: 'thread-2', messages: [] });
    });
    await waitFor(() => expect(screen.queryByText('old-scope.bin')).not.toBeInTheDocument());
    await act(async () => rejectPrompt(new Error('old request failed')));

    await waitFor(() => {
      expect(screen.queryByText('old-scope.bin')).not.toBeInTheDocument();
      expect(screen.getByPlaceholderText('做点什么…')).toHaveValue('');
    });
  });

  it('renders persisted user attachment names in message history', async () => {
    useAcpStore.setState({
      runningByThread: { 'thread-1': false },
      messages: [{
        id: 'user-with-file',
        thread_id: 'thread-1',
        role: 'user',
        content: '请检查这个文件',
        status: 'done',
        attachments: [{
          id: 'stored-file',
          file_type: 'application/x-tar',
          file_name: 'workspace.tar',
          file_path: 'files/stored_workspace.tar',
          file_size: 128,
        }],
        created_at: '2026-08-08T00:00:01Z',
      }],
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_git_info') return { branch: null, branches: [], isRepo: false };
      if (command === 'check_attachment_exists') return true;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    render(
      <App>
        <AcpConversationPane />
      </App>,
    );

    expect(await screen.findByText('请检查这个文件')).toBeInTheDocument();
    expect(await screen.findByText('workspace.tar')).toBeInTheDocument();
  });

  it('remembers spawn-time reasoning choices for future process reuse', async () => {
    const grokSnapshot = {
      ...sessionSnapshot,
      configOptions: sessionSnapshot.configOptions.map((option) =>
        option.id === 'reasoning_effort'
          ? {
              ...option,
              currentValue: 'medium',
              _meta: { aqbotSpawnArg: '--reasoning-effort' },
            }
          : option,
      ),
    };
    useAcpStore.setState({
      sessionByThread: {
        'thread-1': {
          ...sessionSnapshot,
          configOptions: sessionSnapshot.configOptions.map((option) =>
            option.id === 'reasoning_effort'
              ? { ...option, _meta: { aqbotSpawnArg: '--reasoning-effort' } }
              : option,
          ),
        },
      },
      spawnReasoningByThread: {},
    });
    invokeMock.mockResolvedValueOnce(grokSnapshot);

    await act(async () => {
      await useAcpStore.getState().setConfigOption('thread-1', 'reasoning_effort', 'medium');
    });

    expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', {
      threadId: 'thread-1',
      configId: 'reasoning_effort',
      value: 'medium',
    });
    expect(useAcpStore.getState().spawnReasoningByThread['thread-1']).toBe('medium');
  });

  it('remembers discovered spawn-time model choices and can restore the agent default', async () => {
    const modelSnapshot = {
      ...sessionSnapshot,
      configOptions: sessionSnapshot.configOptions.map((option) =>
        option.id === 'model'
          ? { ...option, currentValue: 'gpt-5.6-sol', _meta: { aqbotSpawnArg: '--model' } }
          : option,
      ),
    };
    useAcpStore.setState({
      sessionByThread: {
        'thread-1': {
          ...sessionSnapshot,
          configOptions: sessionSnapshot.configOptions.map((option) =>
            option.id === 'model'
              ? { ...option, _meta: { aqbotSpawnArg: '--model' } }
              : option,
          ),
        },
      },
      spawnModelByThread: {},
    });
    invokeMock.mockResolvedValueOnce(modelSnapshot);

    await act(async () => {
      await useAcpStore.getState().setConfigOption('thread-1', 'model', 'gpt-5.6-sol');
    });
    expect(useAcpStore.getState().spawnModelByThread['thread-1']).toBe('gpt-5.6-sol');

    invokeMock.mockResolvedValueOnce(modelSnapshot);
    await act(async () => {
      await useAcpStore.getState().setConfigOption('thread-1', 'model', '__agent_default');
    });
    expect(useAcpStore.getState().spawnModelByThread['thread-1']).toBeUndefined();
  });

  it('installs the accepted user and assistant rows without reloading a stale message list', async () => {
    useAcpStore.setState({
      messages: [],
      runningByThread: { 'thread-1': false },
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'user-accepted',
            thread_id: 'thread-1',
            role: 'user',
            content: 'hello',
            status: 'done',
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'assistant-accepted',
            thread_id: 'thread-1',
            role: 'assistant',
            content: '',
            status: 'streaming',
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    await act(async () => {
      await useAcpStore.getState().sendPrompt('thread-1', 'hello');
    });

    expect(useAcpStore.getState().messages.map((message) => message.id)).toEqual([
      'user-accepted',
      'assistant-accepted',
    ]);
    expect(useAcpStore.getState().runningByThread['thread-1']).toBe(true);
    expect(invokeMock).not.toHaveBeenCalledWith('acp_list_messages', expect.anything());
  });

  it('ignores an older message-list response that resolves after a prompt is accepted', async () => {
    let resolveOldList!: (messages: []) => void;
    const oldList = new Promise<[]>((resolve) => {
      resolveOldList = resolve;
    });
    useAcpStore.setState({ messages: [], runningByThread: { 'thread-1': false } });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_list_messages') return oldList;
      if (command === 'acp_prompt') {
        return {
          userMessage: {
            id: 'user-new',
            thread_id: 'thread-1',
            role: 'user',
            content: 'new turn',
            status: 'done',
            created_at: '2026-08-08T00:00:01Z',
          },
          assistantMessage: {
            id: 'assistant-new',
            thread_id: 'thread-1',
            role: 'assistant',
            content: '',
            status: 'streaming',
            created_at: '2026-08-08T00:00:02Z',
          },
        };
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const staleRequest = useAcpStore.getState().loadMessages('thread-1');
    await useAcpStore.getState().sendPrompt('thread-1', 'new turn');
    resolveOldList([]);
    await staleRequest;

    expect(useAcpStore.getState().messages.map((message) => message.id)).toEqual([
      'user-new',
      'assistant-new',
    ]);
  });
});
