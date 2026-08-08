import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AcpAgentsFile,
  AcpSessionSnapshot,
  AcpThread,
  RegistryFile,
} from '@/types/acp';

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(async () => vi.fn()),
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

  it('installs a created thread before slow sidebar refreshes finish', async () => {
    const createdThread: AcpThread = {
      ...existingThread,
      id: 'thread-created',
      title: 'First prompt',
    };
    const pendingList = new Promise<AcpThread[]>(() => undefined);
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_create_thread') return createdThread;
      if (command === 'acp_list_threads' || command === 'acp_list_all_threads') {
        return pendingList;
      }
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

    const result = await Promise.race([
      useAcpStore.getState().createThread('project-1', 'grok-build', 'First prompt'),
      new Promise<'timed-out'>((resolve) => setTimeout(() => resolve('timed-out'), 100)),
    ]);

    expect(result).toEqual(createdThread);
    const state = useAcpStore.getState();
    expect(state.activeThreadId).toBe(createdThread.id);
    expect(state.threads).toContainEqual(createdThread);
    expect(state.allThreads).toContainEqual(createdThread);
    expect(state.sessionByThread[draftKey]).toBeUndefined();
    expect(state.sessionByThread[createdThread.id]).toEqual(cachedSession);
    expect(state.spawnModelByThread[createdThread.id]).toBe('grok-4-code');
    expect(state.spawnReasoningByThread[createdThread.id]).toBe('high');
  });
});
