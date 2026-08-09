import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpMessage, AcpPromptAccepted } from '@/types/acp';

type EventHandler = (event: { payload: Record<string, unknown> }) => void;

const { invokeMock, eventHandlers } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  eventHandlers: new Map<string, EventHandler>(),
}));

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
  listen: vi.fn(async (event: string, handler: EventHandler) => {
    eventHandlers.set(event, handler);
    return () => eventHandlers.delete(event);
  }),
}));

function message(
  id: string,
  role: 'user' | 'assistant',
  content: string,
  status: 'done' | 'streaming' | 'error',
): AcpMessage {
  return {
    id,
    thread_id: 'thread-1',
    role,
    content,
    status,
    attachments: [],
    created_at: '2026-08-08T00:00:00Z',
  };
}

describe('acpStore prompt ordering', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    eventHandlers.clear();
    localStorage.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('removes a resolved permission request and records the decision on its tool call', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      pendingPermissions: {
        'permission-1': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'permission-1',
          toolCallId: 'tool-1',
          toolName: 'write_file',
          input: { path: 'README.md' },
          status: 'pending',
          options: [{ id: 'allow_once', label: 'Allow once', variant: 'primary' }],
        },
      },
      toolCalls: {
        'thread-1:assistant-1:tool-1': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-1',
          toolName: 'write_file',
          status: 'queued',
        },
      },
    });

    await useAcpStore.getState().respondPermission('permission-1', 'allow_once');

    expect(invokeMock).toHaveBeenCalledWith('acp_respond_permission', {
      requestId: 'permission-1',
      optionId: 'allow_once',
      feedback: null,
    });
    const state = useAcpStore.getState();
    expect(state.pendingPermissions['permission-1']).toBeUndefined();
    expect(state.toolCalls['thread-1:assistant-1:tool-1']).toMatchObject({
      approvalStatus: 'approved',
      approvalOptionId: 'allow_once',
    });
  });

  it('keeps same-kind tools pending after selecting an agent-advertised AllowAlways option', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();
    useAcpStore.setState({
      pendingPermissions: {
        'permission-1': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'permission-1',
          toolCallId: 'tool-1',
          toolName: 'execute',
          input: { command: 'ls' },
          status: 'pending',
          options: [
            { id: 'allow-once', label: 'Allow once', kind: 'AllowOnce', variant: 'primary' },
            {
              id: 'agent-allow-always',
              label: 'Always allow from Agent',
              kind: 'AllowAlways',
            },
            { id: 'reject-once', label: 'Reject', kind: 'RejectOnce', variant: 'danger' },
          ],
        },
      },
    });

    await useAcpStore.getState().respondPermission('permission-1', 'agent-allow-always');

    expect(invokeMock).toHaveBeenCalledWith('acp_respond_permission', {
      requestId: 'permission-1',
      optionId: 'agent-allow-always',
      feedback: null,
    });
    expect(useAcpStore.getState().pendingPermissions['permission-1']).toBeUndefined();

    invokeMock.mockClear();
    eventHandlers.get('acp-permission-request')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-2',
        requestId: 'permission-2',
        interactionKind: 'permission',
        raw: {
          toolCall: {
            toolCallId: 'tool-2',
            kind: 'execute',
            title: 'Run command',
            rawInput: { command: 'pwd' },
          },
        },
        options: [
          { optionId: 'allow-once', name: 'Allow once', kind: 'AllowOnce' },
          {
            optionId: 'agent-allow-always',
            name: 'Always allow from Agent',
            kind: 'AllowAlways',
          },
          { optionId: 'reject-once', name: 'Reject', kind: 'RejectOnce' },
        ],
      },
    });

    expect(invokeMock).not.toHaveBeenCalledWith(
      'acp_respond_permission',
      expect.objectContaining({ requestId: 'permission-2' }),
    );
    expect(useAcpStore.getState().pendingPermissions['permission-2']).toMatchObject({
      requestId: 'permission-2',
      toolName: 'execute',
      input: { command: 'pwd' },
      status: 'pending',
    });
  });

  it('keeps a no-tool-call permission manual and sends its advertised option unchanged', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-permission-request')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-autohand',
        requestId: 'autohand-permission',
        interactionKind: 'permission',
        toolCallId: null,
        title: 'Allow Autohand to deploy this change?',
        raw: {
          sessionId: 'session-1',
          title: 'Allow Autohand to deploy this change?',
          prompt: 'Select how Autohand should continue.',
          description: 'Autohand needs approval before deployment.',
          tool: 'autohand_permission',
          _meta: {
            title: 'Allow Autohand to deploy this change?',
            prompt: 'Select how Autohand should continue.',
            description: 'Autohand needs approval before deployment.',
            tool: 'autohand_permission',
          },
        },
        options: [
          { optionId: 'allow_once', name: 'Allow once', kind: 'AllowOnce' },
          { optionId: 'allow_always', name: 'Allow always', kind: 'AllowAlways' },
          { optionId: 'reject_once', name: 'Reject', kind: 'RejectOnce' },
        ],
      },
    });

    expect(invokeMock).not.toHaveBeenCalledWith(
      'acp_respond_permission',
      expect.objectContaining({ requestId: 'autohand-permission' }),
    );
    expect(useAcpStore.getState().pendingPermissions['autohand-permission']).toMatchObject({
      title: 'Allow Autohand to deploy this change?',
      toolName: 'autohand_permission',
      kind: 'permission',
    });
    expect(
      useAcpStore.getState().pendingPermissions['autohand-permission']?.toolCallId,
    ).toBeUndefined();

    await useAcpStore.getState().respondPermission('autohand-permission', 'allow_always');

    expect(invokeMock).toHaveBeenCalledWith('acp_respond_permission', {
      requestId: 'autohand-permission',
      optionId: 'allow_always',
      feedback: null,
    });
  });

  it('ignores plan-review documents when updating session plan progress', async () => {
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();
    useAcpStore.setState({
      planByThread: {
        'thread-1': {
          entries: [{ content: 'Real todo', status: 'in_progress' }],
          completed: 0,
          total: 1,
        },
      },
    });

    // Grok exit-plan-mode used to emit acp-plan with planContent markdown —
    // that must never replace structured todos with form-field list noise.
    eventHandlers.get('acp-plan')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        raw: {
          kind: 'plan_review',
          title: 'Plan review',
          planContent: [
            '## Plan',
            '- 用例 ID:',
            '- 语言 / 环境: zh-CN | en-US',
            '- 复现步骤:',
          ].join('\n'),
        },
      },
    });

    expect(useAcpStore.getState().planByThread['thread-1']).toEqual({
      entries: [{ content: 'Real todo', status: 'in_progress' }],
      completed: 0,
      total: 1,
    });
  });

  it('accepts structured session plan entries for progress UI', async () => {
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-plan')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        raw: {
          sessionUpdate: 'plan',
          entries: [
            { content: 'Inspect runtime', status: 'completed' },
            { content: 'Ship fix', status: 'pending', priority: 'high' },
          ],
        },
      },
    });

    expect(useAcpStore.getState().planByThread['thread-1']).toEqual({
      entries: [
        { content: 'Inspect runtime', status: 'completed' },
        { content: 'Ship fix', status: 'pending', priority: 'high' },
      ],
      completed: 1,
      total: 2,
    });
  });

  it('rehydrates plan documents from persisted acp-plan markers on loadMessages', async () => {
    const planBody = '## Plan\n1. Inspect\n2. Ship';
    const marker = [
      '<acp-plan data-aqbot="1" id="plan-1" message="assistant-1" status="approved" title="Plan review">',
      planBody
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;'),
      '</acp-plan>',
    ].join('');
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_list_messages') {
        return [
          message('user-1', 'user', 'plan please', 'done'),
          message(
            'assistant-1',
            'assistant',
            `Before.\n\n${marker}\n\nAfter.`,
            'done',
          ),
        ];
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [],
      planDocumentsByThread: {},
      runningByThread: {},
    });

    await useAcpStore.getState().loadMessages('thread-1');

    expect(useAcpStore.getState().planDocumentsByThread['thread-1']).toEqual([
      expect.objectContaining({
        id: 'plan-1',
        messageId: 'assistant-1',
        content: planBody,
        title: 'Plan review',
        status: 'approved',
      }),
    ]);
  });

  it('keeps plan review documents after approval so the timeline can re-read them', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-permission-request')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        requestId: 'plan-1',
        interactionKind: 'plan_review',
        raw: {
          kind: 'plan_review',
          title: 'Plan review',
          planContent: '## Plan\n1. Inspect\n2. Ship',
        },
        options: [
          { optionId: 'approved', name: 'Approve and implement' },
          { optionId: 'cancelled', name: 'Continue planning' },
          { optionId: 'abandoned', name: 'Abandon plan' },
        ],
      },
    });

    expect(useAcpStore.getState().planDocumentsByThread['thread-1']).toEqual([
      expect.objectContaining({
        id: 'plan-1',
        messageId: 'assistant-1',
        content: '## Plan\n1. Inspect\n2. Ship',
        status: 'pending',
      }),
    ]);

    await useAcpStore.getState().respondPermission('plan-1', 'approved');

    expect(useAcpStore.getState().pendingPermissions['plan-1']).toBeUndefined();
    expect(useAcpStore.getState().planDocumentsByThread['thread-1']).toEqual([
      expect.objectContaining({
        id: 'plan-1',
        status: 'approved',
        content: '## Plan\n1. Inspect\n2. Ship',
      }),
    ]);

    eventHandlers.get('acp-done')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        text: 'done',
      },
    });

    // Turn completion must not wipe the plan body.
    expect(useAcpStore.getState().planDocumentsByThread['thread-1']?.[0]).toMatchObject({
      id: 'plan-1',
      status: 'approved',
      content: '## Plan\n1. Inspect\n2. Ship',
    });
  });

  it('continues Codex plan revision feedback as the next turn', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prompt') {
        return {
          userMessage: message('user-revision', 'user', 'Keep the data path unchanged', 'done'),
          assistantMessage: message('assistant-revision', 'assistant', '', 'streaming'),
        } satisfies AcpPromptAccepted;
      }
      return undefined;
    });
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-permission-request')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-codex-plan',
        requestId: 'codex-plan-review',
        interactionKind: 'permission',
        raw: {
          sessionId: 'session-1',
          toolCall: {
            toolCallId: 'plan-review:item-1',
            title: 'Implement this plan?',
            kind: 'switch_mode',
            status: 'pending',
            rawInput: { plan: '## Codex plan\n1. Inspect\n2. Ship' },
          },
          supportsFeedback: true,
          feedbackDelivery: 'follow_up_prompt',
          _meta: { codex: { kind: 'plan_review', planItemId: 'item-1' } },
        },
        options: [
          { optionId: 'implement_plan', name: 'Yes, implement this plan', kind: 'AllowOnce' },
          {
            optionId: 'revise_plan',
            name: 'No, and tell Codex what to do differently',
            kind: 'RejectOnce',
          },
        ],
      },
    });

    const state = useAcpStore.getState();
    expect(invokeMock).not.toHaveBeenCalledWith(
      'acp_respond_permission',
      expect.objectContaining({ requestId: 'codex-plan-review' }),
    );
    expect(state.pendingPermissions['codex-plan-review']).toMatchObject({
      kind: 'plan_review',
      toolName: 'switch_mode',
      input: {
        plan: '## Codex plan\n1. Inspect\n2. Ship',
        supportsFeedback: true,
        feedbackDelivery: 'follow_up_prompt',
      },
    });
    expect(state.planDocumentsByThread['thread-1']).toEqual([
      expect.objectContaining({
        id: 'codex-plan-review',
        content: '## Codex plan\n1. Inspect\n2. Ship',
        status: 'pending',
      }),
    ]);

    await useAcpStore.getState().respondPermission(
      'codex-plan-review',
      'revise_plan',
      'Keep the data path unchanged',
    );

    expect(invokeMock).toHaveBeenCalledWith('acp_respond_permission', {
      requestId: 'codex-plan-review',
      optionId: 'revise_plan',
      feedback: 'Keep the data path unchanged',
    });
    expect(useAcpStore.getState().planDocumentsByThread['thread-1']?.[0]).toMatchObject({
      status: 'cancelled',
      content: '## Codex plan\n1. Inspect\n2. Ship',
    });
    expect(invokeMock).not.toHaveBeenCalledWith(
      'acp_prompt',
      expect.objectContaining({ prompt: 'Keep the data path unchanged' }),
    );

    eventHandlers.get('acp-done')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-codex-plan',
        text: '',
        stopReason: 'end_turn',
      },
    });

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prompt', expect.objectContaining({
        threadId: 'thread-1',
        prompt: 'Keep the data path unchanged',
      }));
    });
    expect(useAcpStore.getState().planFollowUpByThread['thread-1']).toBeUndefined();
  });

  it('recovers rejected plan revision feedback into its composer draft without losing content', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_respond_permission') return undefined;
      if (command === 'acp_prompt') throw new Error('follow-up unavailable');
      return undefined;
    });
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();
    const existingSnippet = {
      id: 'paste-1',
      content: 'existing pasted content',
      lineCount: 1,
      index: 1,
    };
    const existingFile = new File(['existing'], 'existing.txt', { type: 'text/plain' });
    useAcpStore.setState({
      projects: [{
        id: 'project-1',
        name: 'AQBot',
        root_path: '/tmp/aqbot',
        kind: 'project',
        sort_order: 0,
        created_at: '2026-08-08T00:00:00Z',
        updated_at: '2026-08-08T00:00:00Z',
      }],
      threads: [{
        id: 'thread-1',
        project_id: 'project-1',
        agent_id: 'codex',
        title: 'Plan task',
        runtime_status: 'running',
        is_pinned: 0,
        sort_order: 0,
        created_at: '2026-08-08T00:00:00Z',
        updated_at: '2026-08-08T00:00:00Z',
      }],
      activeProjectId: 'project-1',
      activeThreadId: 'thread-1',
      pendingPermissions: {
        'codex-plan-review': {
          threadId: 'thread-1',
          messageId: 'assistant-codex-plan',
          requestId: 'codex-plan-review',
          kind: 'plan_review',
          toolName: 'switch_mode',
          input: { feedbackDelivery: 'follow_up_prompt' },
          options: [{ id: 'revise_plan', label: 'Revise', kind: 'RejectOnce' }],
          status: 'pending',
        },
      },
      composerDraftsByScope: {
        'project-1:thread-1': {
          value: 'existing composer text',
          snippets: [existingSnippet],
          files: [existingFile],
        },
      },
    });

    await useAcpStore.getState().respondPermission(
      'codex-plan-review',
      'revise_plan',
      'Keep the data path unchanged',
    );
    eventHandlers.get('acp-done')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-codex-plan',
        text: '',
        stopReason: 'end_turn',
      },
    });

    await vi.waitFor(() => {
      expect(useAcpStore.getState().composerDraftsByScope['project-1:thread-1'])
        .toEqual({
          value: 'existing composer text',
          snippets: [existingSnippet],
          files: [existingFile],
          recovery: {
            id: 'codex-plan-review',
            text: 'Keep the data path unchanged',
            error: 'Error: follow-up unavailable',
          },
        });
    });
    expect(useAcpStore.getState().planFollowUpByThread['thread-1']).toBeUndefined();
    expect(invokeMock.mock.calls.filter(([command]) => command === 'acp_prompt')).toHaveLength(1);
  });

  it('keeps unconsumed plan recovery through an empty composer layout snapshot', async () => {
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      projects: [{
        id: 'project-1',
        name: 'AQBot',
        root_path: '/tmp/aqbot',
        kind: 'project',
        sort_order: 0,
        created_at: '2026-08-08T00:00:00Z',
        updated_at: '2026-08-08T00:00:00Z',
      }],
      threads: [{
        id: 'thread-1',
        project_id: 'project-1',
        agent_id: 'codex',
        title: 'Plan task',
        runtime_status: 'idle',
        is_pinned: 0,
        sort_order: 0,
        created_at: '2026-08-08T00:00:00Z',
        updated_at: '2026-08-08T00:00:00Z',
      }],
      composerDraftsByScope: {
        'project-1:thread-1': {
          value: '',
          snippets: [],
          files: [],
          recovery: {
            id: 'codex-plan-review',
            text: 'Keep the data path unchanged',
            error: 'Error: follow-up unavailable',
          },
        },
      },
    });

    useAcpStore.getState().saveComposerDraft('project-1:thread-1', {
      value: '',
      snippets: [],
      files: [],
    });

    expect(useAcpStore.getState().composerDraftsByScope['project-1:thread-1']?.recovery)
      .toEqual({
        id: 'codex-plan-review',
        text: 'Keep the data path unchanged',
        error: 'Error: follow-up unavailable',
      });
  });

  it('cancels a Codex plan review through the ACP cancellation outcome', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();
    useAcpStore.setState({
      pendingPermissions: {
        'codex-plan-review': {
          threadId: 'thread-1',
          messageId: 'assistant-codex-plan',
          requestId: 'codex-plan-review',
          kind: 'plan_review',
          toolName: 'switch_mode',
          input: { plan: '## Codex plan' },
          options: [
            { id: 'implement_plan', label: 'Implement', kind: 'AllowOnce' },
            { id: 'revise_plan', label: 'Revise', kind: 'RejectOnce' },
          ],
          status: 'pending',
        },
      },
      planDocumentsByThread: {
        'thread-1': [{
          id: 'codex-plan-review',
          threadId: 'thread-1',
          messageId: 'assistant-codex-plan',
          content: '## Codex plan',
          status: 'pending',
          sequence: 1,
          createdAt: '2026-08-08T00:00:00Z',
        }],
      },
    });

    await useAcpStore.getState().cancelInteraction('codex-plan-review');

    expect(invokeMock).toHaveBeenCalledWith('acp_cancel_interaction', {
      requestId: 'codex-plan-review',
    });
    expect(useAcpStore.getState().pendingPermissions['codex-plan-review']).toBeUndefined();
    expect(useAcpStore.getState().planDocumentsByThread['thread-1']?.[0].status)
      .toBe('abandoned');

    eventHandlers.get('acp-interaction-closed')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-codex-plan',
        requestId: 'codex-plan-review',
        interactionKind: 'plan_review',
        reason: 'cancelled',
      },
    });

    expect(useAcpStore.getState().planDocumentsByThread['thread-1']?.[0].status)
      .toBe('abandoned');
  });

  it('does not continue plan feedback when the revision decision fails', async () => {
    invokeMock.mockRejectedValue(new Error('agent disconnected'));
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      pendingPermissions: {
        'codex-plan-review': {
          threadId: 'thread-1',
          requestId: 'codex-plan-review',
          kind: 'plan_review',
          toolName: 'switch_mode',
          input: { feedbackDelivery: 'follow_up_prompt' },
          options: [{ id: 'revise_plan', label: 'Revise', kind: 'RejectOnce' }],
          status: 'pending',
        },
      },
    });

    await expect(useAcpStore.getState().respondPermission(
      'codex-plan-review',
      'revise_plan',
      'Keep the data path unchanged',
    )).rejects.toThrow('agent disconnected');

    expect(useAcpStore.getState().planFollowUpByThread['thread-1']).toBeUndefined();
    expect(useAcpStore.getState().pendingPermissions['codex-plan-review']).toBeDefined();
  });

  it('recognizes a metadata-free Claude switch-mode plan and cancels it by option kind', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-permission-request')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-claude-plan',
        requestId: 'claude-plan-review',
        interactionKind: 'permission',
        raw: {
          sessionId: 'session-1',
          toolCall: {
            toolCallId: 'claude-plan-review',
            title: 'Claude plan',
            kind: 'switch_mode',
            status: 'pending',
            rawInput: { plan: '## Claude plan\n1. Inspect\n2. Ship' },
          },
        },
        options: [
          { optionId: 'auto', name: 'Auto mode', kind: 'AllowAlways' },
          { optionId: 'acceptEdits', name: 'Accept edits', kind: 'AllowAlways' },
          { optionId: 'default', name: 'Default mode', kind: 'AllowOnce' },
          { optionId: 'plan', name: 'Keep planning', kind: 'RejectOnce' },
        ],
      },
    });

    expect(invokeMock).not.toHaveBeenCalledWith(
      'acp_respond_permission',
      expect.objectContaining({ requestId: 'claude-plan-review' }),
    );
    expect(useAcpStore.getState().pendingPermissions['claude-plan-review']).toMatchObject({
      kind: 'plan_review',
      toolName: 'switch_mode',
      input: {
        plan: '## Claude plan\n1. Inspect\n2. Ship',
        supportsFeedback: false,
      },
    });

    await useAcpStore.getState().respondPermission('claude-plan-review', 'plan');

    expect(useAcpStore.getState().planDocumentsByThread['thread-1']?.[0]).toMatchObject({
      status: 'cancelled',
      content: '## Claude plan\n1. Inspect\n2. Ship',
    });
  });

  it('keeps a natively cancelled plan review distinct from requested changes', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-permission-request')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        requestId: 'plan-cancelled',
        interactionKind: 'plan_review',
        raw: {
          kind: 'plan_review',
          title: 'Plan review',
          planContent: '## Plan\n1. Keep planning',
        },
        options: [],
      },
    });

    eventHandlers.get('acp-interaction-closed')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        requestId: 'plan-cancelled',
        interactionKind: 'plan_review',
        reason: 'cancelled',
      },
    });

    expect(useAcpStore.getState().planDocumentsByThread['thread-1']?.[0]).toMatchObject({
      id: 'plan-cancelled',
      status: 'abandoned',
    });
  });

  it('submits a structured questionnaire and closes only its composer interaction', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_respond_questionnaire') {
        return 'Which layers?: Frontend, Backend';
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      pendingPermissions: {
        'questionnaire-1': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'questionnaire-1',
          toolCallId: 'tool-questionnaire-1',
          toolName: 'ask_user_question',
          kind: 'question',
          input: { mode: 'default', questions: [] },
          status: 'pending',
          options: [],
        },
      },
      toolCalls: {
        'thread-1:assistant-1:tool-questionnaire-1': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-questionnaire-1',
          toolName: 'ask_user_question',
          status: 'queued',
        },
      },
    });
    const submission = {
      outcome: 'accepted' as const,
      answers: [{
        questionIndex: 0,
        selectedOptionIndexes: [0, 1],
        otherText: '  Keep mobile unchanged  ',
      }],
    };

    await useAcpStore.getState().respondQuestionnaire('questionnaire-1', submission);

    expect(invokeMock).toHaveBeenCalledWith('acp_respond_questionnaire', {
      requestId: 'questionnaire-1',
      outcome: 'accepted',
      answers: submission.answers,
    });
    const state = useAcpStore.getState();
    expect(state.pendingPermissions['questionnaire-1']).toBeUndefined();
    expect(state.toolCalls['thread-1:assistant-1:tool-questionnaire-1']).toMatchObject({
      status: 'queued',
      output: 'Which layers?: Frontend, Backend',
    });
  });

  it('never records a secret questionnaire answer in the tool summary', async () => {
    invokeMock.mockResolvedValue('API token: super-secret-value');
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      pendingPermissions: {
        'questionnaire-secret': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'questionnaire-secret',
          toolCallId: 'tool-questionnaire-secret',
          toolName: 'elicitation_form',
          kind: 'question',
          input: {
            kind: 'elicitation_form',
            questions: [{
              id: 'api_token',
              question: 'API token',
              inputType: 'secret',
              secret: true,
            }],
          },
          status: 'pending',
          options: [],
        },
      },
      toolCalls: {
        'thread-1:assistant-1:tool-questionnaire-secret': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-questionnaire-secret',
          toolName: 'elicitation_form',
          status: 'queued',
        },
      },
    });

    await useAcpStore.getState().respondQuestionnaire('questionnaire-secret', {
      outcome: 'accepted',
      answers: [{
        questionIndex: 0,
        selectedOptionIndexes: [],
        otherText: 'super-secret-value',
      }],
    });

    expect(invokeMock).toHaveBeenCalledWith('acp_respond_questionnaire', {
      requestId: 'questionnaire-secret',
      outcome: 'accepted',
      answers: [{
        questionIndex: 0,
        selectedOptionIndexes: [],
        otherText: 'super-secret-value',
      }],
    });
    expect(
      useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-questionnaire-secret']?.output,
    ).toBe('aqbot:questionnaire:accepted');
  });

  it('hydrates chronological tool results from persisted assistant metadata', async () => {
    const persisted = message(
      'assistant-history',
      'assistant',
      '<tool-call data-aqbot="1" id="tool-7" message="assistant-history" name="terminal">ls</tool-call>',
      'done',
    );
    persisted.meta_json = JSON.stringify({
      duration_ms: 42,
      toolCalls: [{
        toolCallId: 'tool-7',
        toolName: 'terminal',
        status: 'success',
        input: '{"command":"ls"}',
        output: 'README.md',
      }],
    });
    const later = message(
      'assistant-later',
      'assistant',
      '<tool-call data-aqbot="1" id="tool-7" message="assistant-later" name="terminal">pwd</tool-call>',
      'done',
    );
    later.meta_json = JSON.stringify({
      toolCalls: [{
        toolCallId: 'tool-7',
        toolName: 'terminal',
        status: 'success',
        input: '{"command":"pwd"}',
        output: '/workspace',
      }],
    });
    invokeMock.mockResolvedValue([persisted, later]);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      runningByThread: { 'thread-1': false },
      messages: [],
      toolCalls: {},
    });

    await useAcpStore.getState().loadMessages('thread-1');

    expect(useAcpStore.getState().toolCalls['thread-1:assistant-history:tool-7']).toMatchObject({
      messageId: 'assistant-history',
      status: 'success',
      input: '{"command":"ls"}',
      output: 'README.md',
    });
    expect(useAcpStore.getState().toolCalls['thread-1:assistant-later:tool-7']).toMatchObject({
      messageId: 'assistant-later',
      status: 'success',
      output: '/workspace',
    });
  });

  it('closes an interaction from the runtime and uses semantic option kind for denial', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      pendingPermissions: {
        'permission-opaque': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'permission-opaque',
          toolCallId: 'tool-opaque',
          toolName: 'terminal',
          input: { command: 'rm example.txt' },
          status: 'pending',
          options: [{
            id: 'choice-7f2c',
            label: 'No',
            kind: 'RejectOnce',
            variant: 'danger',
          }],
        },
      },
      toolCalls: {},
    });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-interaction-closed')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        requestId: 'permission-opaque',
        interactionKind: 'permission',
        toolCallId: 'tool-opaque',
        reason: 'selected',
        selectedOptionId: 'choice-7f2c',
        selectedOptionKind: 'RejectOnce',
        selectedOptionName: 'No',
      },
    });

    const state = useAcpStore.getState();
    expect(state.pendingPermissions['permission-opaque']).toBeUndefined();
    expect(state.toolCalls['thread-1:assistant-1:tool-opaque']).toMatchObject({
      approvalStatus: 'denied',
      approvalOptionId: 'choice-7f2c',
      approvalLabel: 'No',
      status: 'cancelled',
    });
  });

  it('removes an expired request without misreporting it as a user denial', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      pendingPermissions: {
        'permission-expired': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'permission-expired',
          toolCallId: 'tool-expired',
          toolName: 'terminal',
          input: { command: 'pwd' },
          status: 'pending',
          options: [{ id: 'allow', label: 'Allow', variant: 'primary' }],
        },
      },
      toolCalls: {},
    });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-interaction-closed')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        requestId: 'permission-expired',
        interactionKind: 'permission',
        toolCallId: 'tool-expired',
        reason: 'expired',
      },
    });

    const state = useAcpStore.getState();
    expect(state.pendingPermissions['permission-expired']).toBeUndefined();
    expect(state.toolCalls['thread-1:assistant-1:tool-expired']).toMatchObject({
      approvalStatus: 'expired',
      status: 'cancelled',
    });
    expect(
      state.toolCalls['thread-1:assistant-1:tool-expired'].approvalOptionId,
    ).toBeUndefined();
  });

  it('keeps a selected answer on its tool row while the Agent finishes the tool', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      pendingPermissions: {
        'question-1': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'question-1',
          kind: 'question',
          toolCallId: 'tool-question',
          toolName: 'ask_user_question',
          input: { question: 'Which database?' },
          status: 'pending',
          options: [{ id: 'sqlite', label: 'SQLite', variant: 'default' }],
        },
      },
      toolCalls: {
        'thread-1:assistant-1:tool-question': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-question',
          toolName: 'ask_user_question',
          status: 'queued',
        },
      },
    });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-interaction-closed')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        requestId: 'question-1',
        interactionKind: 'question',
        toolCallId: 'tool-question',
        reason: 'selected',
        selectedOptionId: 'sqlite',
        selectedOptionName: 'SQLite',
      },
    });

    expect(useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-question']).toMatchObject({
      status: 'queued',
      output: 'SQLite',
    });
  });

  it.each([
    ['an existing answer', 'Earlier answer', '', 'Earlier answer'],
    ['no existing answer', undefined, '', 'aqbot:questionnaire:skip_interview'],
    ['an Agent result', 'Canonical Agent result', 'SQLite', 'Canonical Agent result'],
  ])(
    'preserves %s when the interaction closes',
    async (_case, previousOutput, selectedOptionName, expectedOutput) => {
      invokeMock.mockResolvedValue(undefined);
      const { useAcpStore } = await import('../acpStore');
      useAcpStore.setState({
        pendingPermissions: {
          'question-blank': {
            threadId: 'thread-1',
            messageId: 'assistant-1',
            requestId: 'question-blank',
            kind: 'question',
            toolCallId: 'tool-question-blank',
            toolName: 'ask_user_question',
            input: { mode: 'plan', questions: [] },
            status: 'pending',
            options: [],
          },
        },
        toolCalls: {
          'thread-1:assistant-1:tool-question-blank': {
            threadId: 'thread-1',
            messageId: 'assistant-1',
            toolCallId: 'tool-question-blank',
            toolName: 'ask_user_question',
            status: 'queued',
            output: previousOutput,
          },
        },
      });
      await useAcpStore.getState().bindEvents();

      eventHandlers.get('acp-interaction-closed')?.({
        payload: {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          requestId: 'question-blank',
          interactionKind: 'question',
          toolCallId: 'tool-question-blank',
          reason: 'selected',
          selectedOptionId: 'skip_interview',
          selectedOptionName,
        },
      });

      expect(
        useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-question-blank'].output,
      ).toBe(expectedOutput);
    },
  );

  it('replaces a questionnaire placeholder with a canonical rawOutput update', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      toolCalls: {
        'thread-1:assistant-1:tool-question': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-question',
          toolName: 'ask_user_question',
          status: 'queued',
          output: 'aqbot:questionnaire:skip_interview',
        },
      },
    });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-tool-call-update')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        toolCallId: 'tool-question',
        status: 'completed',
        raw: { rawOutput: { outcome: 'skip_interview' } },
      },
    });

    expect(useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-question'])
      .toMatchObject({
        status: 'success',
        output: '{\n  "outcome": "skip_interview"\n}',
      });
  });

  it('captures a canonical output carried by the initial tool-call event', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({ toolCalls: {} });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-tool-call')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        toolCallId: 'tool-question',
        title: 'Ask user',
        status: 'completed',
        raw: { output: 'Agent-recorded result' },
      },
    });

    expect(useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-question'])
      .toMatchObject({
        status: 'success',
        output: 'Agent-recorded result',
      });
  });

  it('preserves a cancelled status from the initial tool-call event', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({ toolCalls: {} });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-tool-call')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        toolCallId: 'tool-cancelled',
        title: 'Run command',
        status: 'cancelled',
        raw: {},
      },
    });

    expect(useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-cancelled'])
      .toMatchObject({ status: 'cancelled' });
  });

  it('stops a running tool when its ACP turn is cancelled', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_list_messages') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      toolCalls: {
        'thread-1:assistant-1:tool-running': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-running',
          toolName: 'execute',
          status: 'running',
        },
      },
    });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-done')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        stopReason: 'cancelled',
        text: '',
      },
    });

    expect(useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-running'])
      .toMatchObject({ status: 'cancelled' });
  });

  it('marks an unfinished tool as failed when the ACP turn errors', async () => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      toolCalls: {
        'thread-1:assistant-1:tool-running': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-running',
          toolName: 'execute',
          status: 'running',
        },
      },
    });
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-error')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        message: 'agent disconnected',
      },
    });

    expect(useAcpStore.getState().toolCalls['thread-1:assistant-1:tool-running'])
      .toMatchObject({ status: 'error' });
  });

  it('keeps a terminal error when an older message load resolves afterward', async () => {
    let resolveMessages!: (messages: AcpMessage[]) => void;
    const pendingMessages = new Promise<AcpMessage[]>((resolve) => {
      resolveMessages = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_list_messages') return pendingMessages;
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [message('assistant-1', 'assistant', 'partial', 'streaming')],
      runningByThread: { 'thread-1': true },
      messagesLoadingByThread: {},
      messagesErrorByThread: {},
      toolCalls: {
        'thread-1:assistant-1:tool-running': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-running',
          toolName: 'execute',
          status: 'running',
        },
      },
    });

    const loading = useAcpStore.getState().loadMessages('thread-1');
    eventHandlers.get('acp-error')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-1',
        message: 'agent disconnected',
        text: 'partial\n\nError: agent disconnected',
      },
    });
    resolveMessages([{
      ...message('assistant-1', 'assistant', 'partial', 'streaming'),
      meta_json: JSON.stringify({
        toolCalls: [{
          toolCallId: 'tool-running',
          toolName: 'execute',
          status: 'running',
        }],
      }),
    }]);
    await loading;

    const state = useAcpStore.getState();
    expect(state.messages[0]).toMatchObject({
      id: 'assistant-1',
      status: 'error',
      content: 'partial\n\nError: agent disconnected',
    });
    expect(state.toolCalls['thread-1:assistant-1:tool-running'])
      .toMatchObject({ status: 'error' });
    expect(state.runningByThread['thread-1']).toBe(false);
    expect(state.messagesLoadingByThread['thread-1']).toBe(false);
  });

  it('restores the prior turn status when cancellation fails', async () => {
    invokeMock.mockRejectedValueOnce(new Error('agent disconnected'));
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      runningByThread: { 'thread-1': true },
      cancellingByThread: {},
      statusByThread: { 'thread-1': 'Working' },
      turnActivityByThread: { 'thread-1': false },
    });

    await expect(useAcpStore.getState().cancelPrompt('thread-1'))
      .rejects.toThrow('agent disconnected');

    expect(useAcpStore.getState()).toMatchObject({
      runningByThread: { 'thread-1': true },
      cancellingByThread: { 'thread-1': false },
      statusByThread: { 'thread-1': 'Working' },
      turnActivityByThread: { 'thread-1': false },
    });
  });

  it('finishes the local turn when cancellation succeeds but history refresh fails', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_cancel') return true;
      if (command === 'acp_list_messages') throw new Error('database unavailable');
      throw new Error(`Unexpected invoke: ${command}`);
    });
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [message('assistant-1', 'assistant', 'partial', 'streaming')],
      streamingText: { 'assistant-1': 'partial' },
      runningByThread: { 'thread-1': true },
      cancellingByThread: {},
      statusByThread: { 'thread-1': 'Working' },
      turnActivityByThread: { 'thread-1': false },
      planByThread: {
        'thread-1': { entries: [{ content: 'Inspect', status: 'pending' }], completed: 0, total: 1 },
      },
      pendingPermissions: {
        'permission-cancelled': {
          threadId: 'thread-1',
          requestId: 'permission-cancelled',
          toolName: 'execute',
          input: {},
          options: [],
          status: 'pending',
        },
      },
      toolCalls: {
        'thread-1:assistant-1:tool-running': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-running',
          toolName: 'execute',
          status: 'running',
        },
      },
      messagesLoadingByThread: {},
      messagesErrorByThread: {},
    });

    await expect(useAcpStore.getState().cancelPrompt('thread-1')).resolves.toBeUndefined();

    const state = useAcpStore.getState();
    expect(state.runningByThread['thread-1']).toBe(false);
    expect(state.cancellingByThread['thread-1']).toBe(false);
    expect(state.statusByThread['thread-1']).toBe('');
    expect(state.turnActivityByThread['thread-1']).toBe(true);
    expect(state.messages[0]).toMatchObject({ status: 'done', content: 'partial' });
    expect(state.streamingText['assistant-1']).toBeUndefined();
    expect(state.planByThread['thread-1']).toEqual({ entries: [], completed: 0, total: 0 });
    expect(state.pendingPermissions['permission-cancelled']).toBeUndefined();
    expect(state.toolCalls['thread-1:assistant-1:tool-running'])
      .toMatchObject({ status: 'cancelled' });
    expect(state.messagesLoadingByThread['thread-1']).toBe(false);
    expect(state.messagesErrorByThread['thread-1']).toContain('database unavailable');
  });

  it('shows an optimistic user row and preserves an error that arrives before the receipt', async () => {
    let resolvePrompt!: (receipt: AcpPromptAccepted) => void;
    const promptReceipt = new Promise<AcpPromptAccepted>((resolve) => {
      resolvePrompt = resolve;
    });
    const accepted: AcpPromptAccepted = {
      userMessage: message('user-real', 'user', 'hello', 'done'),
      assistantMessage: message('assistant-real', 'assistant', '', 'streaming'),
    };
    const persistedError = message('assistant-real', 'assistant', 'Error: disconnected', 'error');
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prompt') return promptReceipt;
      if (command === 'acp_list_messages') return [persistedError];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [],
      runningByThread: {},
      streamingText: {},
      statusByThread: {},
      cancellingByThread: {},
      planByThread: {},
      error: null,
    });

    const sending = useAcpStore.getState().sendPrompt('thread-1', 'hello');
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prompt', expect.anything());
    });
    expect(useAcpStore.getState().messages.some(
      (item) => item.role === 'user' && item.content === 'hello',
    )).toBe(true);

    eventHandlers.get('acp-error')?.({
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-real',
        message: 'disconnected',
        text: 'Error: disconnected',
      },
    });
    resolvePrompt(accepted);
    await sending;

    const state = useAcpStore.getState();
    expect(state.messages.some((item) => item.id.startsWith('optimistic-'))).toBe(false);
    expect(state.messages.find((item) => item.id === 'assistant-real')).toMatchObject({
      content: 'Error: disconnected',
      status: 'error',
    });
    expect(state.runningByThread['thread-1']).toBe(false);
  });

  it('shows a passive first-output hint after twelve silent seconds without stopping the turn', async () => {
    vi.useFakeTimers();
    const accepted: AcpPromptAccepted = {
      userMessage: message('user-real', 'user', 'hello', 'done'),
      assistantMessage: message('assistant-real', 'assistant', '', 'streaming'),
    };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prompt') return accepted;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { ACP_STATUS_FIRST_OUTPUT_SILENCE, useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [],
      runningByThread: {},
      streamingText: {},
      statusByThread: {},
      pendingPermissions: {},
      turnActivityByThread: {},
      error: null,
    });

    await useAcpStore.getState().sendPrompt('thread-1', 'hello');
    await vi.advanceTimersByTimeAsync(11_999);
    expect(useAcpStore.getState().statusByThread['thread-1']).not.toBe(
      ACP_STATUS_FIRST_OUTPUT_SILENCE,
    );

    await vi.advanceTimersByTimeAsync(1);
    expect(useAcpStore.getState().statusByThread['thread-1']).toBe(
      ACP_STATUS_FIRST_OUTPUT_SILENCE,
    );
    expect(useAcpStore.getState().runningByThread['thread-1']).toBe(true);
  });

  it('preserves a meaningful Grok retry status when the silence timer expires', async () => {
    vi.useFakeTimers();
    const accepted: AcpPromptAccepted = {
      userMessage: message('user-real', 'user', 'hello', 'done'),
      assistantMessage: message('assistant-real', 'assistant', '', 'streaming'),
    };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prompt') return accepted;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [],
      runningByThread: {},
      streamingText: {},
      statusByThread: {},
      pendingPermissions: {},
      turnActivityByThread: {},
      error: null,
    });

    await useAcpStore.getState().sendPrompt('thread-1', 'hello');
    const retryStatus = 'aqbot:grok-retry:{"attempt":2,"maximum":5,"detail":"timeout"}';
    eventHandlers.get('acp-status')?.({
      payload: {
        threadId: 'thread-1',
        message: retryStatus,
      },
    });
    await vi.advanceTimersByTimeAsync(12_000);

    expect(useAcpStore.getState().statusByThread['thread-1']).toBe(retryStatus);
    expect(useAcpStore.getState().runningByThread['thread-1']).toBe(true);
  });

  it.each([
    'aqbot:cancel-restarting',
    'aqbot:using-shared-agent',
    'aqbot:launching-agent',
    'aqbot:agent-ready',
    'aqbot:restoring-session',
    'aqbot:saved-session-expired',
    'aqbot:creating-session',
    'aqbot:sending-prompt',
    'aqbot:session-expired',
  ])('keeps runtime-owned status code %s stable', async (message) => {
    invokeMock.mockResolvedValue(undefined);
    const { useAcpStore } = await import('../acpStore');
    await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-status')?.({
      payload: { threadId: 'thread-1', message },
    });

    expect(useAcpStore.getState().statusByThread['thread-1']).toBe(message);
  });

  it('replaces a stable host sending status with the passive silence hint', async () => {
    vi.useFakeTimers();
    const accepted: AcpPromptAccepted = {
      userMessage: message('user-real', 'user', 'hello', 'done'),
      assistantMessage: message('assistant-real', 'assistant', '', 'streaming'),
    };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prompt') return accepted;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { ACP_STATUS_FIRST_OUTPUT_SILENCE, useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [],
      runningByThread: {},
      streamingText: {},
      statusByThread: {},
      pendingPermissions: {},
      turnActivityByThread: {},
      error: null,
    });

    await useAcpStore.getState().sendPrompt('thread-1', 'hello');
    useAcpStore.setState((state) => ({
      statusByThread: {
        ...state.statusByThread,
        'thread-1': 'aqbot:sending-prompt',
      },
    }));
    await vi.advanceTimersByTimeAsync(12_000);

    expect(useAcpStore.getState().statusByThread['thread-1']).toBe(
      ACP_STATUS_FIRST_OUTPUT_SILENCE,
    );
  });

  it.each([
    {
      label: 'stream',
      eventName: 'acp-stream-text',
      payload: { threadId: 'thread-1', messageId: 'assistant-real', text: '' },
    },
    {
      label: 'permission',
      eventName: 'acp-permission-request',
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-real',
        requestId: 'permission-1',
        raw: { toolCall: { kind: 'write_file', rawInput: { path: 'README.md' } } },
        options: [{ optionId: 'allow_once', name: 'Allow once' }],
      },
    },
    {
      label: 'done',
      eventName: 'acp-done',
      payload: { threadId: 'thread-1', messageId: 'assistant-real', text: '' },
    },
    {
      label: 'error',
      eventName: 'acp-error',
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-real',
        message: 'failed',
        text: '',
      },
    },
    {
      label: 'plan',
      eventName: 'acp-plan',
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-real',
        raw: { entries: [{ content: 'Inspect runtime', status: 'in_progress' }] },
      },
    },
    {
      label: 'tool-call',
      eventName: 'acp-tool-call',
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-real',
        toolCallId: 'tool-1',
        title: 'Read file',
        status: 'running',
        raw: { kind: 'read' },
      },
    },
    {
      label: 'tool-call-update',
      eventName: 'acp-tool-call-update',
      payload: {
        threadId: 'thread-1',
        messageId: 'assistant-real',
        toolCallId: 'tool-1',
        status: 'running',
        raw: { content: 'working' },
      },
    },
  ])('disarms the silence hint after the first $label event', async ({ eventName, payload }) => {
    vi.useFakeTimers();
    const accepted: AcpPromptAccepted = {
      userMessage: message('user-real', 'user', 'hello', 'done'),
      assistantMessage: message('assistant-real', 'assistant', '', 'streaming'),
    };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prompt') return accepted;
      if (command === 'acp_list_messages') return [accepted.userMessage, accepted.assistantMessage];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { ACP_STATUS_FIRST_OUTPUT_SILENCE, useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [],
      runningByThread: {},
      streamingText: {},
      statusByThread: {},
      pendingPermissions: {},
      turnActivityByThread: {},
      error: null,
    });

    await useAcpStore.getState().sendPrompt('thread-1', 'hello');
    expect(eventHandlers.get(eventName)).toBeDefined();
    eventHandlers.get(eventName)?.({ payload });
    await vi.advanceTimersByTimeAsync(12_000);

    expect(useAcpStore.getState().turnActivityByThread['thread-1']).toBe(true);
    expect(useAcpStore.getState().statusByThread['thread-1']).not.toBe(
      ACP_STATUS_FIRST_OUTPUT_SILENCE,
    );
  });
});
