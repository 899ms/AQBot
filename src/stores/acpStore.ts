import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { invoke, listen, type UnlistenFn } from '@/lib/invoke';
import type {
  AcpAgentsFile,
  AcpMessage,
  AcpProject,
  AcpThread,
  ConfiguredAgent,
  RegistryFile,
} from '@/types/acp';
import type { PermissionOptionButton } from '@/components/chat/PermissionCard';

// Generation counter prevents StrictMode / remount races from stacking
// multiple acp-stream-text listeners (which doubles every streamed character).
let _acpListenerGen = 0;
let _acpUnlisten: UnlistenFn | null = null;

/** Merge a stream chunk: support both delta and cumulative snapshots; drop exact dupes. */
function mergeStreamChunk(prev: string, chunk: string): string {
  if (!chunk) return prev;
  if (!prev) return chunk;
  // Exact replay of the same payload (double listener / reconnect)
  if (chunk === prev) return prev;
  // Cumulative snapshot (agent sends full text so far each time)
  if (chunk.startsWith(prev) && chunk.length > prev.length) return chunk;
  // Cumulative snapshot that shrank then grew with same prefix path — keep longer
  if (prev.startsWith(chunk)) return prev;
  return prev + chunk;
}

export interface AcpPermissionRequest {
  threadId: string;
  requestId: string;
  toolName: string;
  input: Record<string, unknown>;
  options: PermissionOptionButton[];
  status: 'pending' | 'approved' | 'denied';
  messageId?: string;
}

export interface AcpToolCallState {
  threadId: string;
  messageId?: string;
  toolCallId: string;
  toolName: string;
  status: 'queued' | 'running' | 'success' | 'error' | 'cancelled';
  input?: string;
  output?: string;
}

interface AcpStore {
  config: AcpAgentsFile | null;
  registry: RegistryFile | null;
  projects: AcpProject[];
  threads: AcpThread[];
  messages: AcpMessage[];
  activeProjectId: string | null;
  activeThreadId: string | null;
  streamingText: Record<string, string>;
  statusByThread: Record<string, string>;
  /** threadId → running */
  runningByThread: Record<string, boolean>;
  pendingPermissions: Record<string, AcpPermissionRequest>; // requestId
  toolCalls: Record<string, AcpToolCallState>; // toolCallId
  permissionMode: string;
  loading: boolean;
  error: string | null;
  /**
   * True after the first successful (or failed) config fetch this session.
   * Until then, `agents.length === 0` must NOT be treated as "not configured" —
   * show loading instead (cold start / Tauri not ready yet).
   */
  configReady: boolean;
  /** True after first projects list fetch completes. */
  projectsReady: boolean;
  /** True after first all-threads list fetch completes. */
  threadsReady: boolean;

  loadConfig: () => Promise<void>;
  loadRegistry: (refresh?: boolean) => Promise<void>;
  setAgentEnabled: (agentId: string, enabled: boolean) => Promise<void>;
  addFromRegistry: (agentId: string) => Promise<void>;
  saveGeneral: (general: AcpAgentsFile['general']) => Promise<void>;
  upsertCustom: (agent: ConfiguredAgent) => Promise<void>;
  removeAgent: (agentId: string) => Promise<void>;
  reorderAgents: (agentIds: string[]) => Promise<void>;
  setPermissionMode: (mode: string) => Promise<void>;

  loadProjects: () => Promise<void>;
  /** Optimistic local reorder (like category store onDragOver). */
  setProjectsOrder: (projects: AcpProject[]) => void;
  /** Persist order after drag end. */
  reorderProjects: (projectIds: string[]) => Promise<void>;
  createProject: (name: string, rootPath: string) => Promise<AcpProject>;
  updateProject: (
    projectId: string,
    patch: { name?: string; rootPath?: string },
  ) => Promise<AcpProject>;
  deleteProject: (projectId: string) => Promise<void>;
  selectProject: (projectId: string | null) => Promise<void>;
  loadThreads: (projectId: string) => Promise<void>;
  loadAllThreads: () => Promise<void>;
  createThread: (projectId: string, agentId: string, title?: string) => Promise<AcpThread>;
  deleteThread: (threadId: string) => Promise<void>;
  selectThread: (threadId: string | null) => Promise<void>;
  loadMessages: (threadId: string) => Promise<void>;
  sendPrompt: (threadId: string, prompt: string) => Promise<void>;
  respondPermission: (requestId: string, optionId: string) => Promise<void>;
  /**
   * Re-open the last project + thread after entering the Agent page.
   * Validates ids against the freshly loaded lists, then loads messages.
   */
  restoreLastSession: () => Promise<void>;

  enabledAgents: () => ConfiguredAgent[];
  permissionsForThread: (threadId: string) => AcpPermissionRequest[];
  toolsForThread: (threadId: string) => AcpToolCallState[];
  /** All threads (for sidebar groups under projects). */
  allThreads: AcpThread[];
  bindEvents: () => Promise<UnlistenFn>;
  /** Fire-and-forget warm load used at app bootstrap. */
  warmBootstrap: () => void;
}

function mapPermissionDefaultToMode(value: string | undefined): string {
  if (value === 'full_access') return 'full_access';
  if (value === 'auto_approve') return 'auto_approve';
  if (value === 'accept_edits') return 'accept_edits';
  // legacy "prompt" maps to default
  return 'default';
}

function mapModeToPermissionDefault(mode: string): string {
  if (mode === 'full_access') return 'full_access';
  if (mode === 'auto_approve') return 'auto_approve';
  if (mode === 'accept_edits') return 'accept_edits';
  return 'default';
}

function mapAcpOptions(
  options: Array<{ optionId?: string; option_id?: string; name: string; kind?: string | null }>,
): PermissionOptionButton[] {
  if (!options?.length) {
    return [
      { id: 'allow_once', label: 'Allow Once', variant: 'primary' },
      { id: 'allow_always', label: 'Always Allow', variant: 'default' },
      { id: 'deny', label: 'Deny', variant: 'danger' },
    ];
  }
  return options.map((o) => {
    const id = o.optionId ?? o.option_id ?? o.name;
    const kind = (o.kind ?? '').toLowerCase();
    let variant: PermissionOptionButton['variant'] = 'default';
    if (kind.includes('reject') || kind.includes('deny') || kind.includes('cancel')) {
      variant = 'danger';
    } else if (kind.includes('allow_once') || kind.includes('allowonce')) {
      variant = 'primary';
    } else if (kind.includes('allow')) {
      variant = 'primary';
    }
    return { id, label: o.name || id, variant };
  });
}

function extractToolName(raw: Record<string, unknown>, title?: string | null): string {
  // Prefer short kind (terminal/read/edit) for the chip label — title is often the full command.
  const kind = raw.kind ?? raw.toolKind;
  if (typeof kind === 'string' && kind) return kind;
  if (title && title.length <= 32) return title;
  if (title) return title.split(/\s+/)[0] || 'tool';
  return 'tool';
}

function extractToolInput(raw: Record<string, unknown>): string | undefined {
  const locations = raw.locations ?? raw.content ?? raw.rawInput ?? raw.input;
  if (locations == null) return undefined;
  try {
    return JSON.stringify(locations, null, 2);
  } catch {
    return String(locations);
  }
}

export const useAcpStore = create<AcpStore>()(
  persist(
    (set, get) => ({
  config: null,
  registry: null,
  projects: [],
  threads: [],
  allThreads: [],
  messages: [],
  activeProjectId: null,
  activeThreadId: null,
  streamingText: {},
  statusByThread: {},
  runningByThread: {},
  pendingPermissions: {},
  toolCalls: {},
  permissionMode: 'default',
  loading: false,
  error: null,
  configReady: false,
  projectsReady: false,
  threadsReady: false,

  enabledAgents: () =>
    (get().config?.agents ?? [])
      .filter((a) => a.enabled)
      .slice()
      .sort((a, b) => a.sort - b.sort || a.name.localeCompare(b.name)),

  permissionsForThread: (threadId) =>
    Object.values(get().pendingPermissions).filter((p) => p.threadId === threadId),

  toolsForThread: (threadId) =>
    Object.values(get().toolCalls).filter((t) => t.threadId === threadId),

  loadConfig: async () => {
    try {
      const config = await invoke<AcpAgentsFile>('acp_get_config');
      set({
        config,
        permissionMode: mapPermissionDefaultToMode(config.general?.permissionDefault),
        configReady: true,
        error: null,
      });
    } catch (e) {
      // Keep any cached config so the UI does not flash "not configured".
      set({ configReady: true, error: String(e) });
    }
  },

  loadRegistry: async (refresh = false) => {
    set({ loading: true, error: null });
    try {
      const registry = await invoke<RegistryFile>(
        refresh ? 'acp_refresh_registry' : 'acp_get_registry',
      );
      set({ registry, loading: false });
    } catch (e) {
      set({ loading: false, error: String(e) });
      try {
        const registry = await invoke<RegistryFile>('acp_get_registry');
        set({ registry });
      } catch {
        /* ignore */
      }
    }
  },

  setAgentEnabled: async (agentId, enabled) => {
    const config = await invoke<AcpAgentsFile>('acp_set_agent_enabled', { agentId, enabled });
    set({ config });
  },

  addFromRegistry: async (agentId) => {
    const config = await invoke<AcpAgentsFile>('acp_add_agent_from_registry', {
      agentId,
      enabled: true,
    });
    set({ config });
  },

  saveGeneral: async (general) => {
    const config = await invoke<AcpAgentsFile>('acp_save_general', { general });
    set({
      config,
      permissionMode: mapPermissionDefaultToMode(config.general?.permissionDefault),
    });
  },

  setPermissionMode: async (mode) => {
    const current = get().config;
    if (!current) return;
    const general = {
      ...current.general,
      permissionDefault: mapModeToPermissionDefault(mode),
    };
    await get().saveGeneral(general);
    set({ permissionMode: mode });
  },

  upsertCustom: async (agent) => {
    const config = await invoke<AcpAgentsFile>('acp_upsert_custom_agent', { agent });
    set({ config });
  },

  removeAgent: async (agentId) => {
    const config = await invoke<AcpAgentsFile>('acp_remove_agent', { agentId });
    set({ config });
  },

  reorderAgents: async (agentIds) => {
    const config = await invoke<AcpAgentsFile>('acp_reorder_agents', { agentIds });
    set({ config });
  },

  loadProjects: async () => {
    try {
      const projects = await invoke<AcpProject[]>('acp_list_projects');
      set({ projects, projectsReady: true });
    } catch (e) {
      set({ projectsReady: true, error: String(e) });
    }
  },

  setProjectsOrder: (projects) => {
    set({ projects });
  },

  reorderProjects: async (projectIds) => {
    await invoke('acp_reorder_projects', { projectIds });
    // Keep local sort_order in sync
    set((s) => ({
      projects: projectIds
        .map((id, i) => {
          const p = s.projects.find((x) => x.id === id);
          return p ? { ...p, sort_order: i } : null;
        })
        .filter(Boolean) as AcpProject[],
    }));
  },

  createProject: async (name, rootPath) => {
    const project = await invoke<AcpProject>('acp_create_project', { name, rootPath });
    await get().loadProjects();
    await get().loadAllThreads();
    return project;
  },

  updateProject: async (projectId, patch) => {
    const project = await invoke<AcpProject>('acp_update_project', {
      projectId,
      name: patch.name,
      rootPath: patch.rootPath,
    });
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? project : p)),
    }));
    return project;
  },

  deleteProject: async (projectId) => {
    await invoke('acp_delete_project', { projectId });
    const { activeProjectId } = get();
    if (activeProjectId === projectId) {
      set({ activeProjectId: null, threads: [], activeThreadId: null, messages: [] });
    }
    await get().loadProjects();
    await get().loadAllThreads();
  },

  selectProject: async (projectId) => {
    set({ activeProjectId: projectId, activeThreadId: null, messages: [] });
    if (projectId) {
      await get().loadThreads(projectId);
    } else {
      set({ threads: [] });
    }
  },

  loadThreads: async (projectId) => {
    const threads = await invoke<AcpThread[]>('acp_list_threads', { projectId });
    set((s) => ({
      threads,
      allThreads: [
        ...s.allThreads.filter((th) => th.project_id !== projectId),
        ...threads,
      ],
    }));
  },

  loadAllThreads: async () => {
    try {
      const allThreads = await invoke<AcpThread[]>('acp_list_all_threads');
      const { activeProjectId } = get();
      set({
        allThreads,
        threadsReady: true,
        ...(activeProjectId
          ? { threads: allThreads.filter((th) => th.project_id === activeProjectId) }
          : {}),
      });
    } catch (e) {
      set({ threadsReady: true, error: String(e) });
    }
  },

  warmBootstrap: () => {
    // Fire-and-forget; AgentPage also reloads when opened. Cache makes first paint fast.
    void get().loadConfig();
    void get().loadProjects();
    void get().loadAllThreads();
  },

  createThread: async (projectId, agentId, title) => {
    const thread = await invoke<AcpThread>('acp_create_thread', {
      projectId,
      agentId,
      title: title ?? null,
    });
    await get().loadThreads(projectId);
    await get().loadAllThreads();
    set({
      activeProjectId: projectId,
      activeThreadId: thread.id,
      messages: [],
    });
    return thread;
  },

  deleteThread: async (threadId) => {
    await invoke('acp_delete_thread', { threadId });
    const { activeProjectId, activeThreadId } = get();
    if (activeThreadId === threadId) {
      set({ activeThreadId: null, messages: [] });
    }
    if (activeProjectId) await get().loadThreads(activeProjectId);
    await get().loadAllThreads();
  },

  selectThread: async (threadId) => {
    if (!threadId) {
      set({ activeThreadId: null, messages: [] });
      return;
    }
    const thread =
      get().allThreads.find((th) => th.id === threadId)
      ?? get().threads.find((th) => th.id === threadId)
      ?? null;

    if (thread) {
      const needsProjectSwitch = get().activeProjectId !== thread.project_id;
      set({
        activeProjectId: thread.project_id,
        activeThreadId: threadId,
        messages: [],
        ...(needsProjectSwitch
          ? {
              threads: get().allThreads.filter((th) => th.project_id === thread.project_id),
            }
          : {}),
      });
      if (needsProjectSwitch) {
        await get().loadThreads(thread.project_id);
      }
    } else {
      set({ activeThreadId: threadId, messages: [] });
    }
    await get().loadMessages(threadId);
  },

  restoreLastSession: async () => {
    const { activeProjectId, activeThreadId, projects, allThreads } = get();
    if (!activeProjectId && !activeThreadId) return;

    let projectId = activeProjectId;
    let threadId = activeThreadId;

    // Prefer the last thread; derive project from it when still present.
    if (threadId) {
      const thread = allThreads.find((th) => th.id === threadId);
      if (!thread) {
        threadId = null;
        set({ activeThreadId: null, messages: [] });
      } else {
        projectId = thread.project_id;
      }
    }

    if (projectId && !projects.some((p) => p.id === projectId)) {
      set({
        activeProjectId: null,
        activeThreadId: null,
        messages: [],
        threads: [],
      });
      return;
    }

    if (projectId) {
      set({
        activeProjectId: projectId,
        ...(threadId ? { activeThreadId: threadId } : {}),
        threads: allThreads.filter((th) => th.project_id === projectId),
      });
      try {
        await get().loadThreads(projectId);
      } catch {
        /* keep cached threads for project */
      }
    }

    if (threadId) {
      // Re-validate after loadThreads (thread may have been deleted server-side)
      const stillThere =
        get().allThreads.some((th) => th.id === threadId)
        || get().threads.some((th) => th.id === threadId);
      if (!stillThere) {
        set({ activeThreadId: null, messages: [] });
        return;
      }
      set({ activeThreadId: threadId });
      await get().loadMessages(threadId);
    }
  },

  loadMessages: async (threadId) => {
    const messages = await invoke<AcpMessage[]>('acp_list_messages', { threadId });
    // If a turn is still running, preserve local streaming content so a mid-turn
    // reload does not wipe the live buffer or revive a stuck spinner.
    set((s) => {
      if (s.activeThreadId !== threadId) return s;
      if (!s.runningByThread[threadId]) {
        return { messages };
      }
      const localById = new Map(s.messages.map((m) => [m.id, m]));
      const merged = messages.map((m) => {
        const local = localById.get(m.id);
        if (!local) return m;
        const streamed = s.streamingText[m.id];
        if (streamed && streamed.length >= (m.content?.length ?? 0)) {
          return { ...m, content: streamed, status: 'streaming' as const };
        }
        if ((local.content?.length ?? 0) > (m.content?.length ?? 0)) {
          return { ...m, content: local.content, status: local.status ?? m.status };
        }
        return m;
      });
      return { messages: merged };
    });
  },

  sendPrompt: async (threadId, prompt) => {
    set((s) => ({
      runningByThread: { ...s.runningByThread, [threadId]: true },
      error: null,
    }));
    try {
      // acp_prompt returns as soon as the turn is scheduled. Do NOT loadMessages
      // here — the assistant row is still status=streaming and would thrash the UI.
      // Stream + acp-done events own the live message state; acp-done loads after persist.
      await invoke('acp_prompt', { threadId, prompt });
      // Seed local list with the just-created rows only if this thread is active.
      if (get().activeThreadId === threadId) {
        await get().loadMessages(threadId);
      }
    } catch (e) {
      set((s) => ({
        runningByThread: { ...s.runningByThread, [threadId]: false },
        error: String(e),
      }));
      throw e;
    }
  },

  respondPermission: async (requestId, optionId) => {
    await invoke('acp_respond_permission', { requestId, optionId });
    set((s) => {
      const existing = s.pendingPermissions[requestId];
      if (!existing) return s;
      const denied =
        optionId.toLowerCase().includes('reject')
        || optionId.toLowerCase().includes('deny')
        || optionId.toLowerCase().includes('cancel');
      return {
        pendingPermissions: {
          ...s.pendingPermissions,
          [requestId]: {
            ...existing,
            status: denied ? 'denied' : 'approved',
          },
        },
      };
    });
  },

  bindEvents: async () => {
    // Tear down any previous generation first (StrictMode remount / leave+reenter Agent).
    const gen = ++_acpListenerGen;
    if (_acpUnlisten) {
      _acpUnlisten();
      _acpUnlisten = null;
    }

    const unlisteners: UnlistenFn[] = [];
    const isLive = () => _acpListenerGen === gen;

    unlisteners.push(
      await listen<{ threadId: string; messageId: string; text: string }>(
        'acp-stream-text',
        (event) => {
          if (!isLive()) return;
          const { threadId, messageId, text } = event.payload;
          set((s) => {
            // Ignore late chunks after this turn has already finished.
            if (s.runningByThread[threadId] === false && !s.streamingText[messageId]) {
              const existing = s.messages.find((m) => m.id === messageId);
              if (existing && (existing.status === 'done' || existing.status === 'error')) {
                return s;
              }
            }
            const prev = s.streamingText[messageId] ?? '';
            const nextStream = mergeStreamChunk(prev, text ?? '');
            if (nextStream === prev && s.messages.some((m) => m.id === messageId)) {
              return s;
            }
            const hasMsg = s.messages.some((m) => m.id === messageId);
            return {
              runningByThread: { ...s.runningByThread, [threadId]: true },
              streamingText: {
                ...s.streamingText,
                [messageId]: nextStream,
              },
              messages: hasMsg
                ? s.messages.map((m) =>
                    m.id === messageId
                      ? { ...m, content: nextStream, status: 'streaming' }
                      : m,
                  )
                : s.activeThreadId === threadId
                  ? [
                      ...s.messages,
                      {
                        id: messageId,
                        thread_id: threadId,
                        role: 'assistant',
                        content: nextStream,
                        status: 'streaming',
                        created_at: new Date().toISOString(),
                      } as AcpMessage,
                    ]
                  : s.messages,
            };
          });
        },
      ),
    );

    unlisteners.push(
      await listen<{ threadId: string; message: string }>('acp-status', (event) => {
        if (!isLive()) return;
        const { threadId, message } = event.payload;
        set((s) => ({
          statusByThread: { ...s.statusByThread, [threadId]: message },
          runningByThread: { ...s.runningByThread, [threadId]: true },
        }));
      }),
    );

    unlisteners.push(
      await listen<{
        threadId: string;
        messageId?: string;
        requestId: string;
        raw: Record<string, unknown>;
        options: Array<{ optionId?: string; option_id?: string; name: string; kind?: string }>;
      }>('acp-permission-request', (event) => {
        if (!isLive()) return;
        const { threadId, messageId, requestId, raw, options } = event.payload;
        const toolCall = (raw.toolCall ?? raw.tool_call ?? raw) as Record<string, unknown>;
        const toolName =
          (typeof toolCall.kind === 'string' && toolCall.kind)
          || (typeof toolCall.toolName === 'string' && toolCall.toolName)
          || (typeof toolCall.title === 'string' && String(toolCall.title).slice(0, 40))
          || 'tool';
        const inputObj =
          (toolCall.rawInput as Record<string, unknown>)
          || (toolCall.input as Record<string, unknown>)
          || toolCall;
        set((s) => ({
          pendingPermissions: {
            ...s.pendingPermissions,
            [requestId]: {
              threadId,
              messageId,
              requestId,
              toolName: String(toolName),
              input: typeof inputObj === 'object' && inputObj ? inputObj : { value: inputObj },
              options: mapAcpOptions(options ?? []),
              status: 'pending',
            },
          },
        }));
      }),
    );

    unlisteners.push(
      await listen<{
        threadId: string;
        messageId?: string;
        toolCallId: string;
        title?: string | null;
        kind?: string | null;
        status?: string | null;
        raw: Record<string, unknown>;
      }>('acp-tool-call', (event) => {
        if (!isLive()) return;
        const p = event.payload;
        const statusRaw = (p.status ?? 'pending').toLowerCase();
        const status: AcpToolCallState['status'] =
          statusRaw === 'completed' || statusRaw === 'success'
            ? 'success'
            : statusRaw === 'failed' || statusRaw === 'error'
              ? 'error'
              : statusRaw === 'in_progress' || statusRaw === 'running'
                ? 'running'
                : 'queued';
        set((s) => ({
          toolCalls: {
            ...s.toolCalls,
            [p.toolCallId]: {
              threadId: p.threadId,
              messageId: p.messageId,
              toolCallId: p.toolCallId,
              toolName: extractToolName(p.raw ?? {}, p.title),
              status,
              input: extractToolInput(p.raw ?? {}),
            },
          },
        }));
      }),
    );

    unlisteners.push(
      await listen<{
        threadId: string;
        messageId?: string;
        toolCallId: string;
        status?: string | null;
        raw: Record<string, unknown>;
      }>('acp-tool-call-update', (event) => {
        if (!isLive()) return;
        const p = event.payload;
        set((s) => {
          const existing = s.toolCalls[p.toolCallId];
          const statusRaw = (p.status ?? existing?.status ?? 'running').toLowerCase();
          const status: AcpToolCallState['status'] =
            statusRaw === 'completed' || statusRaw === 'success'
              ? 'success'
              : statusRaw === 'failed' || statusRaw === 'error'
                ? 'error'
                : statusRaw === 'cancelled'
                  ? 'cancelled'
                  : statusRaw === 'in_progress' || statusRaw === 'running'
                    ? 'running'
                    : 'queued';
          const content = p.raw?.content;
          let output: string | undefined;
          if (content != null) {
            try {
              output = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
            } catch {
              output = String(content);
            }
          }
          return {
            toolCalls: {
              ...s.toolCalls,
              [p.toolCallId]: {
                threadId: p.threadId,
                messageId: p.messageId ?? existing?.messageId,
                toolCallId: p.toolCallId,
                toolName: existing?.toolName ?? extractToolName(p.raw ?? {}),
                status,
                input: existing?.input ?? extractToolInput(p.raw ?? {}),
                output: output ?? existing?.output,
              },
            },
          };
        });
      }),
    );

    unlisteners.push(
      await listen<{
        threadId: string;
        messageId: string;
        text: string;
        sessionId?: string;
        durationMs?: number;
      }>(
        'acp-done',
        (event) => {
          if (!isLive()) return;
          const { threadId, messageId, text, durationMs } = event.payload;
          const metaJson =
            typeof durationMs === 'number'
              ? JSON.stringify({ duration_ms: Math.round(durationMs) })
              : undefined;
          set((s) => {
            const nextStreaming = { ...s.streamingText };
            const streamed = nextStreaming[messageId] ?? '';
            delete nextStreaming[messageId];
            const finalContent = (text && text.length > 0 ? text : streamed)
              || s.messages.find((m) => m.id === messageId)?.content
              || '';
            const hasMsg = s.messages.some((m) => m.id === messageId);
            const patch = {
              content: finalContent,
              status: 'done' as const,
              ...(metaJson ? { meta_json: metaJson } : {}),
            };
            const messages = hasMsg
              ? s.messages.map((m) =>
                  m.id === messageId ? { ...m, ...patch } : m,
                )
              : s.activeThreadId === threadId
                ? [
                    ...s.messages,
                    {
                      id: messageId,
                      thread_id: threadId,
                      role: 'assistant',
                      content: finalContent,
                      status: 'done',
                      meta_json: metaJson ?? null,
                      created_at: new Date().toISOString(),
                    } as AcpMessage,
                  ]
                : s.messages;
            return {
              streamingText: nextStreaming,
              statusByThread: { ...s.statusByThread, [threadId]: '' },
              runningByThread: { ...s.runningByThread, [threadId]: false },
              messages,
            };
          });
          // DB is already status=done when this event fires — safe to resync.
          if (get().activeThreadId === threadId) {
            void get().loadMessages(threadId);
          }
        },
      ),
    );

    unlisteners.push(
      await listen<{ threadId: string; messageId?: string; message: string; text?: string }>(
        'acp-error',
        (event) => {
          if (!isLive()) return;
          const { threadId, messageId, message, text } = event.payload;
          set((s) => {
            const nextStreaming = { ...s.streamingText };
            if (messageId) delete nextStreaming[messageId];
            return {
              streamingText: nextStreaming,
              statusByThread: { ...s.statusByThread, [threadId]: message },
              runningByThread: { ...s.runningByThread, [threadId]: false },
              error: message,
              messages: messageId
                ? s.messages.map((m) =>
                    m.id === messageId
                      ? {
                          ...m,
                          content: text || m.content || `Error: ${message}`,
                          status: 'error',
                        }
                      : m,
                  )
                : s.messages,
            };
          });
          if (get().activeThreadId === threadId) {
            void get().loadMessages(threadId);
          }
        },
      ),
    );

    // If a newer bindEvents started while we were awaiting listen(), drop these.
    if (!isLive()) {
      unlisteners.forEach((u) => u());
      return () => {};
    }

    const cleanup = () => {
      unlisteners.forEach((u) => u());
      if (_acpUnlisten === cleanup) {
        _acpUnlisten = null;
      }
    };
    _acpUnlisten = cleanup;
    return cleanup;
  },
    }),
    {
      name: 'aqbot-acp-cache',
      // Instant paint after cold start: show last-known agents/projects while revalidating.
      // Ready flags are NOT persisted — each session still revalidates from backend.
      // activeProjectId / activeThreadId are persisted so re-entering Agent restores
      // the last open project conversation.
      partialize: (s) => ({
        config: s.config,
        permissionMode: s.permissionMode,
        projects: s.projects,
        allThreads: s.allThreads,
        activeProjectId: s.activeProjectId,
        activeThreadId: s.activeThreadId,
      }),
    },
  ),
);
