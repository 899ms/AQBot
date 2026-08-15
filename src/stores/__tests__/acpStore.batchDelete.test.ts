import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AcpProject, AcpThread } from '@/types/acp';

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
  listen: vi.fn(async () => () => undefined),
}));

function project(overrides: Partial<AcpProject> = {}): AcpProject {
  return {
    id: 'project-1',
    name: 'Project',
    root_path: '/tmp/project',
    kind: 'project',
    sort_order: 0,
    created_at: '2026-08-08T00:00:00Z',
    updated_at: '2026-08-08T00:00:00Z',
    ...overrides,
  };
}

function thread(overrides: Partial<AcpThread> = {}): AcpThread {
  return {
    id: 'thread-1',
    project_id: 'project-1',
    agent_id: 'grok-build',
    title: 'Thread 1',
    runtime_status: 'idle',
    mode_id: null,
    is_pinned: 0,
    sort_order: 0,
    created_at: '2026-08-08T00:00:00Z',
    updated_at: '2026-08-08T00:00:00Z',
    ...overrides,
  };
}

describe('acpStore batchDeleteThreads', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    });
  });

  it('deletes every selected thread and reloads the remaining lists once', async () => {
    const kept = thread({ id: 'thread-kept', title: 'Kept' });
    const first = thread({ id: 'thread-1', title: 'First' });
    const second = thread({ id: 'thread-2', title: 'Second', sort_order: 1 });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_delete_thread') return undefined;
      if (command === 'acp_list_threads') return [kept];
      if (command === 'acp_list_all_threads') return [kept];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'project-1',
      activeThreadId: 'thread-1',
      projects: [project()],
      threads: [first, second, kept],
      allThreads: [first, second, kept],
      composerDraftsByScope: {
        'project-1:thread-1': { value: 'draft-1', snippets: [], files: [] },
        'project-1:thread-2': { value: 'draft-2', snippets: [], files: [] },
        'project-1:thread-kept': { value: 'keep', snippets: [], files: [] },
      },
      runningByThread: { 'thread-1': true, 'thread-kept': true },
    });

    await useAcpStore.getState().batchDeleteThreads(['thread-1', 'thread-2']);

    expect(invokeMock.mock.calls.filter(([command]) => command === 'acp_delete_thread')).toEqual([
      ['acp_delete_thread', { threadId: 'thread-1' }],
      ['acp_delete_thread', { threadId: 'thread-2' }],
    ]);
    expect(invokeMock.mock.calls.filter(([command]) => command === 'acp_list_all_threads')).toHaveLength(1);
    expect(invokeMock.mock.calls.filter(([command]) => command === 'acp_list_threads')).toHaveLength(1);

    const state = useAcpStore.getState();
    expect(state.threads.map((item) => item.id)).toEqual(['thread-kept']);
    expect(state.allThreads.map((item) => item.id)).toEqual(['thread-kept']);
    expect(state.activeThreadId).toBeNull();
    expect(state.composerDraftsByScope['project-1:thread-1']).toBeUndefined();
    expect(state.composerDraftsByScope['project-1:thread-2']).toBeUndefined();
    expect(state.composerDraftsByScope['project-1:thread-kept']).toEqual({
      value: 'keep',
      snippets: [],
      files: [],
    });
    expect(state.runningByThread['thread-1']).toBeUndefined();
    expect(state.runningByThread['thread-kept']).toBe(true);
  });

  it('drops recent projects whose threads were deleted', async () => {
    const recentA = project({
      id: 'recent-a',
      name: 'Recent A',
      kind: 'recent',
      root_path: '/tmp/recent-a',
    });
    const recentB = project({
      id: 'recent-b',
      name: 'Recent B',
      kind: 'recent',
      root_path: '/tmp/recent-b',
      sort_order: 1,
    });
    const workspace = project({ id: 'project-1', name: 'Workspace' });
    const threadA = thread({ id: 'thread-a', project_id: 'recent-a', title: 'A' });
    const threadB = thread({ id: 'thread-b', project_id: 'recent-b', title: 'B' });
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'acp_delete_thread') return undefined;
      if (command === 'acp_list_all_threads') return [];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'recent-a',
      activeThreadId: 'thread-a',
      projects: [workspace, recentA, recentB],
      threads: [threadA],
      allThreads: [threadA, threadB],
    });

    await useAcpStore.getState().batchDeleteThreads(['thread-a', 'thread-b']);

    const state = useAcpStore.getState();
    expect(state.projects.map((item) => item.id)).toEqual(['project-1']);
    expect(state.activeProjectId).toBeNull();
    expect(state.threads).toEqual([]);
    expect(state.allThreads).toEqual([]);
  });

  it('keeps surviving threads when one delete fails', async () => {
    const first = thread({ id: 'thread-1' });
    const second = thread({ id: 'thread-2', sort_order: 1 });
    invokeMock.mockImplementation(async (command: string, args?: { threadId?: string }) => {
      if (command === 'acp_delete_thread') {
        if (args?.threadId === 'thread-2') throw new Error('delete failed');
        return undefined;
      }
      if (command === 'acp_list_threads') return [second];
      if (command === 'acp_list_all_threads') return [second];
      throw new Error(`Unexpected invoke: ${command}`);
    });

    const { useAcpStore } = await import('../acpStore');
    useAcpStore.setState({
      activeProjectId: 'project-1',
      activeThreadId: 'thread-2',
      projects: [project()],
      threads: [first, second],
      allThreads: [first, second],
      error: null,
    });

    await useAcpStore.getState().batchDeleteThreads(['thread-1', 'thread-2']);

    const state = useAcpStore.getState();
    expect(state.threads.map((item) => item.id)).toEqual(['thread-2']);
    expect(state.allThreads.map((item) => item.id)).toEqual(['thread-2']);
    expect(state.activeThreadId).toBe('thread-2');
    expect(state.error).toContain('delete failed');
  });
});
