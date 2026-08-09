import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AcpAgentsFile,
  AcpProject,
  AcpSessionSnapshot,
  AcpThread,
  RegistryFile,
} from '@/types/acp';

type EventHandler = (event: { payload: Record<string, unknown> }) => void;

const { invokeMock, listenMock, eventHandlers } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  eventHandlers: new Map<string, EventHandler>(),
  listenMock: vi.fn(async (eventName: string, handler: EventHandler) => {
    eventHandlers.set(eventName, handler);
    return () => eventHandlers.delete(eventName);
  }),
}));

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
  listen: listenMock,
}));

const config: AcpAgentsFile = {
  general: {
    idleTimeoutSecs: 300,
    maxConcurrentProcesses: 0,
    permissionDefault: 'default',
    registryRefresh: 'on_start',
  },
  agents: [{
    id: 'grok-build',
    name: 'Grok Build',
    enabled: true,
    source: 'registry',
    command: 'grok',
    args: ['acp'],
    sort: 0,
  }],
};

const registry: RegistryFile = {
  version: '1',
  source: 'live',
  agents: [{ id: 'grok-build', name: 'Grok Build', version: '1.0.0' }],
};

const cachedSession: AcpSessionSnapshot = {
  sessionId: 'session-cached',
  modes: null,
  configOptions: [],
  agentCapabilities: {},
};

const existingThread: AcpThread = {
  id: 'thread-1',
  project_id: 'project-1',
  agent_id: 'grok-build',
  title: 'Existing thread',
  runtime_status: 'idle',
  mode_id: null,
  is_pinned: 0,
  sort_order: 0,
  created_at: '2026-08-08T00:00:00Z',
  updated_at: '2026-08-08T00:00:00Z',
};

describe('acpStore lifecycle', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventHandlers.clear();
    localStorage.clear();
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
  });

  it('coalesces StrictMode bootstrap and prewarms before a slow Registry refresh', async () => {
    let finishRegistryRefresh!: () => void;
    const pendingRegistryRefresh = new Promise<void>((resolve) => {
      finishRegistryRefresh = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_get_config') return config;
      if (command === 'acp_list_projects' || command === 'acp_list_all_threads') return [];
      if (command === 'acp_refresh_registry') {
        await pendingRegistryRefresh;
        return registry;
      }
      if (command === 'acp_prewarm_enabled_agents') {
        return [{ agentId: 'grok-build', ready: true }];
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.getState().warmBootstrap();
    useAcpStore.getState().warmBootstrap();

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prewarm_enabled_agents');
    });

    // Agent readiness must not wait for the optional network refresh.
    expect(useAcpStore.getState().agentReadinessById['grok-build']).toEqual({
      status: 'ready',
      error: null,
    });
    finishRegistryRefresh();
    await vi.waitFor(() => {
      expect(useAcpStore.getState().registry).toEqual(registry);
    });

    const commands = invokeMock.mock.calls.map(([command]) => command);
    expect(commands.filter((command) => command === 'acp_refresh_registry')).toHaveLength(1);
    expect(commands.filter((command) => command === 'acp_prewarm_enabled_agents')).toHaveLength(1);
    expect(commands).not.toContain('acp_prepare_draft');
  });

  it('coalesces overlapping prewarm requests from config changes', async () => {
    let finishPrewarm!: () => void;
    const pendingPrewarm = new Promise<void>((resolve) => {
      finishPrewarm = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_set_agent_enabled') return config;
      if (command === 'acp_prewarm_enabled_agents') {
        await pendingPrewarm;
        return [{ agentId: 'grok-build', ready: false, error: 'authentication required' }];
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    await Promise.all([
      useAcpStore.getState().setAgentEnabled('grok-build', true),
      useAcpStore.getState().setAgentEnabled('grok-build', true),
    ]);

    const prewarmCalls = invokeMock.mock.calls.filter(
      ([command]) => command === 'acp_prewarm_enabled_agents',
    );
    expect(prewarmCalls).toHaveLength(1);
    finishPrewarm();
    await vi.waitFor(() => {
      expect(useAcpStore.getState().agentReadinessById['grok-build']).toEqual({
        status: 'error',
        error: 'authentication required',
      });
    });
  });

  it('does not let an older config failure replace a newer successful load', async () => {
    const requests: Array<{
      resolve: (value: AcpAgentsFile) => void;
      reject: (error: Error) => void;
    }> = [];
    invokeMock.mockImplementation(async (command: string) => {
      if (command !== 'acp_get_config') throw new Error(`Unexpected invoke: ${command}`);
      return new Promise<AcpAgentsFile>((resolve, reject) => {
        requests.push({ resolve, reject });
      });
    });
    const latestConfig: AcpAgentsFile = {
      ...config,
      agents: [{ ...config.agents[0], name: 'Latest Agent' }],
    };

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({ config: null, configError: null, configReady: false });
    const olderLoad = useAcpStore.getState().loadConfig();
    const newerLoad = useAcpStore.getState().loadConfig();
    await vi.waitFor(() => expect(requests).toHaveLength(2));

    requests[1].resolve(latestConfig);
    await newerLoad;
    requests[0].reject(new Error('older config read failed'));
    await olderLoad;

    expect(useAcpStore.getState()).toMatchObject({
      config: latestConfig,
      configReady: true,
      configError: null,
    });
  });

  it('does not let an older config load replace a successful config mutation', async () => {
    let resolveOlderLoad!: (value: AcpAgentsFile) => void;
    const olderLoadResult = new Promise<AcpAgentsFile>((resolve) => {
      resolveOlderLoad = resolve;
    });
    const updatedConfig: AcpAgentsFile = {
      ...config,
      agents: [{ ...config.agents[0], name: 'Updated Agent' }],
    };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_get_config') return olderLoadResult;
      if (command === 'acp_set_agent_enabled') return updatedConfig;
      if (command === 'acp_prewarm_enabled_agents') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({ config, configError: null, configReady: true });
    const loading = useAcpStore.getState().loadConfig();
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_get_config');
    });
    await useAcpStore.getState().setAgentEnabled('grok-build', true);
    resolveOlderLoad(config);
    await loading;

    expect(useAcpStore.getState().config).toEqual(updatedConfig);
    expect(useAcpStore.getState().configError).toBeNull();
  });

  it('selects a thread with a cached session without preparing it again', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_list_messages') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'project-1',
      activeThreadId: null,
      threads: [existingThread],
      allThreads: [existingThread],
      messages: [],
      sessionByThread: { 'thread-1': cachedSession },
      preparingByThread: {},
    });

    await useAcpStore.getState().selectThread('thread-1');

    expect(useAcpStore.getState().activeThreadId).toBe('thread-1');
    expect(invokeMock).toHaveBeenCalledWith('acp_list_messages', { threadId: 'thread-1' });
    expect(invokeMock).not.toHaveBeenCalledWith('acp_prepare_session', expect.anything());
  });

  it('does not resurrect a session when preparation finishes after thread deletion', async () => {
    let resolvePreparation!: (snapshot: AcpSessionSnapshot) => void;
    const preparation = new Promise<AcpSessionSnapshot>((resolve) => {
      resolvePreparation = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prepare_session') return preparation;
      if (command === 'acp_delete_thread') return undefined;
      if (command === 'acp_list_threads' || command === 'acp_list_all_threads') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'project-1',
      activeThreadId: 'thread-1',
      threads: [existingThread],
      allThreads: [existingThread],
      messages: [{
        id: 'assistant-deleted',
        thread_id: 'thread-1',
        role: 'assistant',
        content: 'partial',
        status: 'streaming',
        attachments: [],
        created_at: '2026-08-08T00:00:00Z',
      }],
      streamingText: { 'assistant-deleted': 'partial' },
      sessionByThread: {},
      preparingByThread: {},
      runningByThread: { 'thread-1': true },
      statusByThread: { 'thread-1': 'Preparing' },
      turnActivityByThread: { 'thread-1': true },
      cancellingByThread: { 'thread-1': false },
      planByThread: {
        'thread-1': { entries: [{ content: 'Inspect', status: 'pending' }], completed: 0, total: 1 },
      },
      composerDraftsByScope: {
        'project-1:thread-1': { value: 'deleted draft', snippets: [], files: [] },
      },
    });

    const preparing = useAcpStore.getState().prepareSession('thread-1')
      .catch(() => undefined);
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prepare_session', expect.anything());
    });
    await useAcpStore.getState().deleteThread('thread-1');
    resolvePreparation(cachedSession);
    await preparing;

    const state = useAcpStore.getState();
    expect(state.sessionByThread['thread-1']).toBeUndefined();
    expect(state.preparingByThread['thread-1']).toBeUndefined();
    expect(state.runningByThread['thread-1']).toBeUndefined();
    expect(state.statusByThread['thread-1']).toBeUndefined();
    expect(state.turnActivityByThread['thread-1']).toBeUndefined();
    expect(state.cancellingByThread['thread-1']).toBeUndefined();
    expect(state.planByThread['thread-1']).toBeUndefined();
    expect(state.composerDraftsByScope['project-1:thread-1']).toBeUndefined();
    expect(state.activeThreadId).toBeNull();
    expect(state.threads).toEqual([]);
    expect(state.allThreads).toEqual([]);
    expect(state.messages).toEqual([]);
    expect(state.streamingText['assistant-deleted']).toBeUndefined();

    useAcpStore.getState().saveComposerDraft('project-1:thread-1', {
      value: 'late deleted thread draft',
      snippets: [],
      files: [],
    });
    expect(useAcpStore.getState().composerDraftsByScope['project-1:thread-1'])
      .toBeUndefined();
  });

  it('ignores every late thread event after the thread is deleted', async () => {
    const deletedThread = { ...existingThread, id: 'thread-deleted' };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_delete_thread') return undefined;
      if (command === 'acp_list_threads' || command === 'acp_list_all_threads') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: deletedThread.project_id,
      activeThreadId: null,
      threads: [deletedThread],
      allThreads: [deletedThread],
      messages: [],
      sessionByThread: {},
      statusByThread: {},
      runningByThread: {},
      pendingPermissions: {},
      toolCalls: {},
      planByThread: {},
      streamingText: {},
    });
    const cleanup = await useAcpStore.getState().bindEvents();

    eventHandlers.get('acp-stream-text')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-flushed-before-delete',
        text: 'flushed text',
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(
      useAcpStore.getState().streamingText['assistant-flushed-before-delete'],
    ).toBe('flushed text');

    eventHandlers.get('acp-stream-text')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-queued-before-delete',
        text: 'queued text',
      },
    });
    await useAcpStore.getState().deleteThread(deletedThread.id);

    eventHandlers.get('acp-session-state')?.({
      payload: { threadId: deletedThread.id, snapshot: cachedSession },
    });
    eventHandlers.get('acp-status')?.({
      payload: { threadId: deletedThread.id, message: 'late status' },
    });
    eventHandlers.get('acp-plan')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-late',
        raw: { entries: [{ content: 'late plan', status: 'pending' }] },
      },
    });
    eventHandlers.get('acp-permission-request')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-late',
        requestId: 'permission-late',
        raw: { toolCall: { toolCallId: 'tool-late', kind: 'execute' } },
        options: [{ optionId: 'allow', name: 'Allow', kind: 'AllowOnce' }],
      },
    });
    eventHandlers.get('acp-tool-call')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-late',
        toolCallId: 'tool-late',
        status: 'running',
        raw: {},
      },
    });
    eventHandlers.get('acp-stream-text')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-late',
        text: 'late text',
      },
    });
    eventHandlers.get('acp-done')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-late',
        text: 'late text',
      },
    });
    eventHandlers.get('acp-error')?.({
      payload: {
        threadId: deletedThread.id,
        messageId: 'assistant-late',
        message: 'late error',
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 20));

    const state = useAcpStore.getState();
    expect(state.sessionByThread[deletedThread.id]).toBeUndefined();
    expect(state.statusByThread[deletedThread.id]).toBeUndefined();
    expect(state.runningByThread[deletedThread.id]).toBeUndefined();
    expect(state.planByThread[deletedThread.id]).toBeUndefined();
    expect(state.pendingPermissions['permission-late']).toBeUndefined();
    expect(state.toolCalls[`${deletedThread.id}:assistant-late:tool-late`]).toBeUndefined();
    expect(state.streamingText['assistant-late']).toBeUndefined();
    expect(state.streamingText['assistant-queued-before-delete']).toBeUndefined();
    expect(state.streamingText['assistant-flushed-before-delete']).toBeUndefined();
    expect(state.messages).toEqual([]);
    cleanup();
  });

  it('retires every child thread and draft when a project is deleted', async () => {
    const projectId = 'project-deleted';
    const threadId = 'thread-project-deleted';
    const draftKey = `draft:${projectId}:grok-build`;
    const deletedProject: AcpProject = {
      id: projectId,
      name: 'Deleted project',
      root_path: '/tmp/project-deleted',
      kind: 'project',
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    };
    const deletedThread: AcpThread = {
      ...existingThread,
      id: threadId,
      project_id: projectId,
    };
    let resolveThreadPreparation!: (snapshot: AcpSessionSnapshot) => void;
    let resolveDraftPreparation!: (snapshot: AcpSessionSnapshot) => void;
    const threadPreparation = new Promise<AcpSessionSnapshot>((resolve) => {
      resolveThreadPreparation = resolve;
    });
    const draftPreparation = new Promise<AcpSessionSnapshot>((resolve) => {
      resolveDraftPreparation = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_prepare_session') return threadPreparation;
      if (command === 'acp_prepare_draft') return draftPreparation;
      if (command === 'acp_delete_project') return undefined;
      if (command === 'acp_list_projects' || command === 'acp_list_all_threads') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      projects: [deletedProject],
      activeProjectId: projectId,
      activeThreadId: threadId,
      threads: [deletedThread],
      allThreads: [deletedThread],
      messages: [{
        id: 'assistant-project-deleted',
        thread_id: threadId,
        role: 'assistant',
        content: 'partial',
        status: 'streaming',
        attachments: [],
        created_at: '2026-08-08T00:00:00Z',
      }],
      streamingText: { 'assistant-project-deleted': 'partial' },
      sessionByThread: { [threadId]: cachedSession, [draftKey]: cachedSession },
      preparingByThread: {},
      runningByThread: { [threadId]: true },
      statusByThread: { [threadId]: 'Working' },
      turnActivityByThread: { [threadId]: true },
      cancellingByThread: { [threadId]: false },
      planByThread: {
        [threadId]: { entries: [{ content: 'Inspect', status: 'pending' }], completed: 0, total: 1 },
      },
      planDocumentsByThread: { [threadId]: [] },
      composerDraftsByScope: {
        [`${projectId}:draft`]: { value: 'project draft', snippets: [], files: [] },
        [`${projectId}:${threadId}`]: { value: 'thread draft', snippets: [], files: [] },
        'recent:draft': { value: 'unrelated draft', snippets: [], files: [] },
      },
      spawnModelByThread: { [threadId]: 'model-a', [draftKey]: 'model-a' },
      spawnReasoningByThread: { [threadId]: 'high', [draftKey]: 'high' },
      messagesLoadingByThread: { [threadId]: true },
      messagesErrorByThread: { [threadId]: 'old load failure' },
      pendingPermissions: {
        'permission-project-deleted': {
          threadId,
          requestId: 'permission-project-deleted',
          toolName: 'execute',
          input: {},
          options: [],
          status: 'pending',
        },
      },
      toolCalls: {
        [`${threadId}:assistant-project-deleted:tool-1`]: {
          threadId,
          messageId: 'assistant-project-deleted',
          toolCallId: 'tool-1',
          toolName: 'execute',
          status: 'running',
        },
      },
      error: null,
    });

    const preparingThread = useAcpStore.getState().prepareSession(threadId);
    const preparingDraft = useAcpStore.getState().prepareDraft(projectId, 'grok-build');
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_prepare_session', expect.anything());
      expect(invokeMock).toHaveBeenCalledWith('acp_prepare_draft', expect.anything());
    });
    await useAcpStore.getState().deleteProject(projectId);
    resolveThreadPreparation(cachedSession);
    resolveDraftPreparation(cachedSession);
    await Promise.allSettled([preparingThread, preparingDraft]);

    const state = useAcpStore.getState();
    expect(state.projects).toEqual([]);
    expect(state.threads).toEqual([]);
    expect(state.allThreads).toEqual([]);
    expect(state.activeProjectId).toBeNull();
    expect(state.activeThreadId).toBeNull();
    expect(state.messages).toEqual([]);
    expect(state.streamingText['assistant-project-deleted']).toBeUndefined();
    for (const key of [threadId, draftKey]) {
      expect(state.sessionByThread[key]).toBeUndefined();
      expect(state.preparingByThread[key]).toBeUndefined();
      expect(state.spawnModelByThread[key]).toBeUndefined();
      expect(state.spawnReasoningByThread[key]).toBeUndefined();
    }
    expect(state.runningByThread[threadId]).toBeUndefined();
    expect(state.statusByThread[threadId]).toBeUndefined();
    expect(state.turnActivityByThread[threadId]).toBeUndefined();
    expect(state.cancellingByThread[threadId]).toBeUndefined();
    expect(state.planByThread[threadId]).toBeUndefined();
    expect(state.planDocumentsByThread[threadId]).toBeUndefined();
    expect(state.composerDraftsByScope[`${projectId}:draft`]).toBeUndefined();
    expect(state.composerDraftsByScope[`${projectId}:${threadId}`]).toBeUndefined();
    expect(state.composerDraftsByScope['recent:draft']?.value).toBe('unrelated draft');
    expect(state.messagesLoadingByThread[threadId]).toBeUndefined();
    expect(state.messagesErrorByThread[threadId]).toBeUndefined();
    expect(state.pendingPermissions['permission-project-deleted']).toBeUndefined();
    expect(state.toolCalls[`${threadId}:assistant-project-deleted:tool-1`]).toBeUndefined();
    expect(state.error).toBeNull();

    useAcpStore.getState().saveComposerDraft(`${projectId}:draft`, {
      value: 'late deleted project draft',
      snippets: [],
      files: [],
    });
    expect(useAcpStore.getState().composerDraftsByScope[`${projectId}:draft`])
      .toBeUndefined();
  });

  it('accepts unsent drafts for live project, thread, and Recent scopes', async () => {
    const liveProject: AcpProject = {
      id: 'project-live',
      name: 'Live project',
      root_path: '/tmp/project-live',
      kind: 'project',
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    };
    const liveThread: AcpThread = {
      ...existingThread,
      id: 'thread-live',
      project_id: liveProject.id,
    };
    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      projects: [liveProject],
      threads: [liveThread],
      allThreads: [liveThread],
      activeProjectId: liveProject.id,
      activeThreadId: liveThread.id,
      composerDraftsByScope: {},
    });

    useAcpStore.getState().saveComposerDraft(`${liveProject.id}:draft`, {
      value: 'project draft',
      snippets: [],
      files: [],
    });
    useAcpStore.getState().saveComposerDraft(`${liveProject.id}:${liveThread.id}`, {
      value: 'thread draft',
      snippets: [],
      files: [],
    });
    useAcpStore.getState().saveComposerDraft('recent:draft', {
      value: 'Recent draft',
      snippets: [],
      files: [],
    });

    expect(useAcpStore.getState().composerDraftsByScope).toMatchObject({
      [`${liveProject.id}:draft`]: { value: 'project draft' },
      [`${liveProject.id}:${liveThread.id}`]: { value: 'thread draft' },
      'recent:draft': { value: 'Recent draft' },
    });
  });

  it('does not restore a late configuration snapshot after thread deletion', async () => {
    const configuredSession: AcpSessionSnapshot = {
      ...cachedSession,
      configOptions: [{
        id: 'reasoning_effort',
        name: 'Reasoning effort',
        category: 'thought_level',
        type: 'select',
        currentValue: 'high',
        options: [{ value: 'high', name: 'High' }],
      }],
    };
    let resolveConfiguration!: (snapshot: AcpSessionSnapshot) => void;
    const configuration = new Promise<AcpSessionSnapshot>((resolve) => {
      resolveConfiguration = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_set_config_option') return configuration;
      if (command === 'acp_delete_thread') return undefined;
      if (command === 'acp_list_threads' || command === 'acp_list_all_threads') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'project-1',
      activeThreadId: 'thread-1',
      threads: [existingThread],
      allThreads: [existingThread],
      sessionByThread: { 'thread-1': cachedSession },
    });

    const updating = useAcpStore.getState().setConfigOption(
      'thread-1',
      'reasoning_effort',
      'high',
    );
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('acp_set_config_option', expect.anything());
    });
    await useAcpStore.getState().deleteThread('thread-1');
    resolveConfiguration(configuredSession);
    await updating;

    expect(useAcpStore.getState().sessionByThread['thread-1']).toBeUndefined();
  });

  it('distinguishes message loading, failure, and a successfully loaded history', async () => {
    let rejectMessages!: (error: Error) => void;
    const pendingMessages = new Promise<never>((_, reject) => {
      rejectMessages = reject;
    });
    let listCalls = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_list_messages') {
        listCalls += 1;
        if (listCalls === 1) return pendingMessages;
        return [{
          id: 'user-loaded',
          thread_id: 'thread-1',
          role: 'user',
          content: 'Persisted history',
          status: 'done',
          attachments: [],
          created_at: '2026-08-08T00:00:01Z',
        }];
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      messages: [],
      messagesLoadingByThread: {},
      messagesErrorByThread: {},
    });

    const firstLoad = useAcpStore.getState().loadMessages('thread-1');
    expect(useAcpStore.getState().messagesLoadingByThread['thread-1']).toBe(true);
    rejectMessages(new Error('database unavailable'));
    await firstLoad;
    expect(useAcpStore.getState().messagesLoadingByThread['thread-1']).toBe(false);
    expect(useAcpStore.getState().messagesErrorByThread['thread-1'])
      .toContain('database unavailable');

    await useAcpStore.getState().loadMessages('thread-1');
    expect(useAcpStore.getState().messagesLoadingByThread['thread-1']).toBe(false);
    expect(useAcpStore.getState().messagesErrorByThread['thread-1']).toBeUndefined();
    expect(useAcpStore.getState().messages).toEqual([
      expect.objectContaining({ id: 'user-loaded', content: 'Persisted history' }),
    ]);
  });

  it('keeps the flat Recent conversation order stable when selecting a thread', async () => {
    const firstRecent: AcpThread = {
      ...existingThread,
      id: 'recent-first',
      project_id: 'recent-project-first',
      title: 'First recent conversation',
    };
    const secondRecent: AcpThread = {
      ...existingThread,
      id: 'recent-second',
      project_id: 'recent-project-second',
      title: 'Second recent conversation',
    };
    invokeMock.mockImplementation(async (command: string, args?: { projectId?: string }) => {
      if (command === 'acp_list_threads' && args?.projectId === firstRecent.project_id) {
        return [firstRecent];
      }
      if (command === 'acp_list_messages') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: null,
      activeThreadId: null,
      projects: [],
      threads: [],
      allThreads: [firstRecent, secondRecent],
      messages: [],
      sessionByThread: {
        [firstRecent.id]: cachedSession,
        [secondRecent.id]: cachedSession,
      },
    });

    await useAcpStore.getState().selectThread(firstRecent.id);

    expect(useAcpStore.getState().allThreads.map((thread) => thread.id)).toEqual([
      firstRecent.id,
      secondRecent.id,
    ]);
  });

  it('never shows the previous project threads while the next project loads', async () => {
    const nextThread: AcpThread = {
      ...existingThread,
      id: 'thread-2',
      project_id: 'project-2',
      title: 'Next project thread',
    };
    let resolveThreads!: (threads: AcpThread[]) => void;
    const pendingThreads = new Promise<AcpThread[]>((resolve) => {
      resolveThreads = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_list_threads') return pendingThreads;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'project-1',
      threads: [existingThread],
      allThreads: [existingThread, nextThread],
    });

    const selecting = useAcpStore.getState().selectProject('project-2');

    expect(useAcpStore.getState().activeProjectId).toBe('project-2');
    expect(useAcpStore.getState().threads).toEqual([nextThread]);
    resolveThreads([nextThread]);
    await selecting;
  });

  it('does not restore an old thread after the user navigates to another project', async () => {
    const project = (id: string): AcpProject => ({
      id,
      name: id,
      root_path: `/tmp/${id}`,
      kind: 'project',
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    });
    const nextThread: AcpThread = {
      ...existingThread,
      id: 'thread-2',
      project_id: 'project-2',
      title: 'Next project thread',
    };
    let resolveOldThreads!: (threads: AcpThread[]) => void;
    const oldThreads = new Promise<AcpThread[]>((resolve) => {
      resolveOldThreads = resolve;
    });
    invokeMock.mockImplementation(async (command: string, args?: { projectId?: string }) => {
      if (command === 'acp_list_threads' && args?.projectId === 'project-1') return oldThreads;
      if (command === 'acp_list_threads' && args?.projectId === 'project-2') return [nextThread];
      if (command === 'acp_list_messages') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      projects: [project('project-1'), project('project-2')],
      activeProjectId: 'project-1',
      activeThreadId: 'thread-1',
      threads: [existingThread],
      allThreads: [existingThread, nextThread],
      messages: [],
    });

    const restoring = useAcpStore.getState().restoreLastSession();
    await useAcpStore.getState().selectProject('project-2');
    resolveOldThreads([existingThread]);
    await restoring;

    expect(useAcpStore.getState()).toMatchObject({
      activeProjectId: 'project-2',
      activeThreadId: null,
      threads: [nextThread],
      messages: [],
    });
  });

  it('coalesces Recent draft creation without overriding a newer selection', async () => {
    const recentDraft: AcpProject = {
      id: 'recent-draft',
      name: 'New conversation',
      root_path: '/tmp/recent-draft',
      kind: 'recent_draft',
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    };
    let resolveDraft!: (project: AcpProject) => void;
    let resolveProjects!: (projects: AcpProject[]) => void;
    const pendingDraft = new Promise<AcpProject>((resolve) => {
      resolveDraft = resolve;
    });
    const pendingProjects = new Promise<AcpProject[]>((resolve) => {
      resolveProjects = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_ensure_recent_draft') return pendingDraft;
      if (command === 'acp_list_projects') return pendingProjects;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      projects: [],
      activeProjectId: null,
      activeThreadId: null,
      threads: [],
      messages: [],
    });

    const staleProjectLoad = useAcpStore.getState().loadProjects();
    const first = useAcpStore.getState().ensureRecentDraft();
    const second = useAcpStore.getState().ensureRecentDraft();
    useAcpStore.setState({
      activeProjectId: 'newer-project',
      activeThreadId: 'newer-thread',
    });
    resolveDraft(recentDraft);
    await Promise.all([first, second]);
    resolveProjects([]);
    await staleProjectLoad;

    expect(invokeMock.mock.calls.filter(
      ([command]) => command === 'acp_ensure_recent_draft',
    )).toHaveLength(1);
    expect(useAcpStore.getState().projects).toContainEqual(recentDraft);
    expect(useAcpStore.getState().projectsReady).toBe(true);
    expect(useAcpStore.getState().activeProjectId).toBe('newer-project');
    expect(useAcpStore.getState().activeThreadId).toBe('newer-thread');
  });

  it('preserves Recent projects and drafts when user projects are reordered', async () => {
    const project = (id: string, kind: AcpProject['kind']): AcpProject => ({
      id,
      name: id,
      root_path: `/tmp/${id}`,
      kind,
      sort_order: 0,
      created_at: '2026-08-08T00:00:00Z',
      updated_at: '2026-08-08T00:00:00Z',
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_reorder_projects') return undefined;
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      projects: [
        project('project-a', 'project'),
        project('project-b', 'project'),
        project('recent-thread-project', 'recent'),
        project('recent-draft-project', 'recent_draft'),
      ],
    });

    await useAcpStore.getState().reorderProjects(['project-b', 'project-a']);

    expect(useAcpStore.getState().projects.map(({ id }) => id)).toEqual([
      'project-b',
      'project-a',
      'recent-thread-project',
      'recent-draft-project',
    ]);
  });

  it('installs a created thread before slow sidebar refreshes finish', async () => {
    const createdThread: AcpThread = {
      ...existingThread,
      id: 'thread-created',
      title: 'First prompt',
    };
    let resolveStaleList!: (threads: AcpThread[]) => void;
    const staleList = new Promise<AcpThread[]>((resolve) => {
      resolveStaleList = resolve;
    });
    let projectListCalls = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_create_thread') return createdThread;
      if (command === 'acp_list_threads') {
        projectListCalls += 1;
        return projectListCalls === 1 ? staleList : [createdThread];
      }
      if (command === 'acp_list_all_threads') return [createdThread];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    const draftKey = 'draft:project-1:grok-build';
    useAcpStore.setState({
      activeProjectId: 'project-1',
      activeThreadId: null,
      threads: [],
      allThreads: [],
      messages: [],
      sessionByThread: { [draftKey]: cachedSession },
      spawnModelByThread: { [draftKey]: 'grok-4-code' },
      spawnReasoningByThread: { [draftKey]: 'high' },
    });
    const staleLoad = useAcpStore.getState().loadThreads('project-1');

    const result = await useAcpStore
      .getState()
      .createThread('project-1', 'grok-build', 'First prompt');
    resolveStaleList([]);
    await staleLoad;

    expect(result).toEqual(createdThread);
    const state = useAcpStore.getState();
    expect(state.activeThreadId).toBe(createdThread.id);
    expect(state.threads).toContainEqual(createdThread);
    expect(state.allThreads).toContainEqual(createdThread);
    expect(state.sessionByThread[draftKey]).toBeUndefined();
    expect(state.sessionByThread[createdThread.id]).toEqual(cachedSession);
    expect(state.spawnModelByThread[createdThread.id]).toBe('grok-4-code');
    expect(state.spawnReasoningByThread[createdThread.id]).toBe('high');
    expect(state.creatingThread).toBe(false);
  });

  it('does not let a late first-thread receipt override a newer selection', async () => {
    const createdThread: AcpThread = {
      ...existingThread,
      id: 'thread-created-late',
      title: 'First prompt',
    };
    let resolveCreate!: (thread: AcpThread) => void;
    const pendingCreate = new Promise<AcpThread>((resolve) => {
      resolveCreate = resolve;
    });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_create_thread') return pendingCreate;
      if (command === 'acp_list_threads' || command === 'acp_list_all_threads') {
        return [createdThread];
      }
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'project-1',
      activeThreadId: null,
      threads: [],
      allThreads: [],
      messages: [],
      sessionByThread: { 'draft:project-1:grok-build': cachedSession },
    });

    const creating = useAcpStore
      .getState()
      .createThread('project-1', 'grok-build', 'First prompt');
    expect(useAcpStore.getState().creatingThread).toBe(true);
    useAcpStore.setState({
      activeProjectId: 'newer-project',
      activeThreadId: 'newer-thread',
    });
    resolveCreate(createdThread);
    await creating;

    expect(useAcpStore.getState().activeProjectId).toBe('newer-project');
    expect(useAcpStore.getState().activeThreadId).toBe('newer-thread');
    expect(useAcpStore.getState().allThreads).toContainEqual(createdThread);
    expect(useAcpStore.getState().creatingThread).toBe(false);
  });
});
