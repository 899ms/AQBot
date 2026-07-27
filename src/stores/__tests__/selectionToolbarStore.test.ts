import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();
const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    return () => listeners.delete(event);
  }),
}));

describe('selection toolbar store', () => {
  beforeEach(() => {
    listeners.clear();
    vi.clearAllMocks();
    vi.resetModules();
  });

  it('ignores stale run chunks and resets result state for a new selection', async () => {
    invokeMock.mockResolvedValue({
      runtime: {
        state: 'running',
        platform: 'macos',
        permission: 'granted',
        last_error: null,
        global_dismissal_supported: true,
      },
      session: {
        selection_id: 'selection-1',
        tools: [],
        theme: 'light',
        language: 'en-US',
      },
      run: null,
    });
    const { useSelectionToolbarStore } = await import('../selectionToolbarStore');
    await useSelectionToolbarStore.getState().initialize();

    listeners.get('selection-toolbar://run')?.({
      payload: {
        kind: 'started',
        request_id: 'request-1',
        selection_id: 'selection-1',
        tool_id: 'summarize',
      },
    });
    listeners.get('selection-toolbar://run')?.({
      payload: { kind: 'delta', request_id: 'request-1', selection_id: 'selection-1', delta: 'kept' },
    });
    listeners.get('selection-toolbar://run')?.({
      payload: { kind: 'delta', request_id: 'request-old', selection_id: 'selection-1', delta: 'ignored' },
    });
    expect(useSelectionToolbarStore.getState().run?.output).toBe('kept');

    listeners.get('selection-toolbar://session')?.({
      payload: {
        selection_id: 'selection-2',
        tools: [],
        theme: 'dark',
        language: 'zh-CN',
      },
    });
    expect(useSelectionToolbarStore.getState().session?.selection_id).toBe('selection-2');
    expect(useSelectionToolbarStore.getState().run).toBeNull();
  });

  it('refreshes the active session without discarding an in-flight run', async () => {
    invokeMock.mockResolvedValue({
      runtime: {
        state: 'running',
        platform: 'macos',
        permission: 'granted',
        last_error: null,
        global_dismissal_supported: true,
      },
      session: {
        selection_id: 'selection-1',
        tools: [],
        theme: 'light',
        language: 'en-US',
      },
      run: {
        request_id: 'request-1',
        selection_id: 'selection-1',
        tool_id: 'summarize',
        status: 'streaming',
        output: 'partial',
        error: null,
      },
    });
    const { useSelectionToolbarStore } = await import('../selectionToolbarStore');
    await useSelectionToolbarStore.getState().initialize();

    listeners.get('selection-toolbar://session')?.({
      payload: {
        selection_id: 'selection-1',
        tools: [{ id: 'copy', kind: 'action', icon: 'copy', label_key: 'copy' }],
        theme: 'dark',
        language: 'zh-CN',
      },
    });

    expect(useSelectionToolbarStore.getState().run?.output).toBe('partial');
    expect(useSelectionToolbarStore.getState().surface).toBe('result');
    expect(useSelectionToolbarStore.getState().session?.theme).toBe('dark');
  });

  it('shows execution preflight failures in the result surface', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'selection_toolbar_get_snapshot') {
        return {
          runtime: {
            state: 'running',
            platform: 'macos',
            permission: 'granted',
            last_error: null,
            global_dismissal_supported: true,
          },
          session: {
            selection_id: 'selection-1',
            tools: [],
            theme: 'light',
            language: 'en-US',
          },
          run: null,
        };
      }
      if (command === 'selection_toolbar_execute_tool') {
        throw new Error('Configured model is disabled');
      }
      return undefined;
    });
    const { useSelectionToolbarStore } = await import('../selectionToolbarStore');
    await useSelectionToolbarStore.getState().initialize();

    await useSelectionToolbarStore.getState().executeTool({
      id: 'translate',
      kind: 'ai',
      icon: 'languages',
      builtin_key: 'translate',
      name: null,
    });

    expect(useSelectionToolbarStore.getState().surface).toBe('result');
    expect(useSelectionToolbarStore.getState().run).toMatchObject({
      selection_id: 'selection-1',
      tool_id: 'translate',
      status: 'error',
      output: '',
      error: 'Error: Configured model is disabled',
    });
  });

  it('does not let an older snapshot overwrite a session event received during startup', async () => {
    let resolveSnapshot!: (value: unknown) => void;
    invokeMock.mockImplementation((command: string) => {
      if (command !== 'selection_toolbar_get_snapshot') return Promise.resolve(undefined);
      return new Promise((resolve) => {
        resolveSnapshot = resolve;
      });
    });
    const { useSelectionToolbarStore } = await import('../selectionToolbarStore');
    const initializing = useSelectionToolbarStore.getState().initialize();
    await Promise.resolve();
    await Promise.resolve();

    listeners.get('selection-toolbar://session')?.({
      payload: {
        selection_id: 'selection-new',
        tools: [],
        theme: 'dark',
        language: 'zh-CN',
      },
    });
    resolveSnapshot({
      runtime: {
        state: 'running',
        platform: 'macos',
        permission: 'granted',
        last_error: null,
        global_dismissal_supported: true,
      },
      session: {
        selection_id: 'selection-old',
        tools: [],
        theme: 'light',
        language: 'en-US',
      },
      run: null,
    });
    await initializing;

    expect(useSelectionToolbarStore.getState().session?.selection_id).toBe('selection-new');
    expect(useSelectionToolbarStore.getState().runtime.state).toBe('running');
  });

  it('re-runs translate with the panel languages and persists target changes', async () => {
    const translateTool = {
      id: 'translate',
      kind: 'ai' as const,
      icon: 'languages',
      builtin_key: 'translate' as const,
      name: null,
    };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'selection_toolbar_get_snapshot') {
        return {
          runtime: {
            state: 'running',
            platform: 'macos',
            permission: 'granted',
            last_error: null,
            global_dismissal_supported: true,
          },
          session: {
            selection_id: 'selection-1',
            tools: [translateTool],
            theme: 'light',
            language: 'en-US',
            translate_target_language: null,
          },
          run: null,
        };
      }
      if (command === 'selection_toolbar_execute_tool') return 'request-9';
      return undefined;
    });
    const { useSelectionToolbarStore } = await import('../selectionToolbarStore');
    await useSelectionToolbarStore.getState().initialize();

    await useSelectionToolbarStore.getState().setTranslateLanguages('en', 'ja');

    expect(invokeMock).toHaveBeenCalledWith('selection_toolbar_set_translate_target', {
      language: 'ja',
    });
    expect(invokeMock).toHaveBeenCalledWith('selection_toolbar_execute_tool', {
      selectionId: 'selection-1',
      toolId: 'translate',
      options: { source_language: 'en', target_language: 'ja' },
    });

    // A plain re-click on the translate tool keeps the chosen languages.
    invokeMock.mockClear();
    invokeMock.mockImplementation(async (command: string) =>
      command === 'selection_toolbar_execute_tool' ? 'request-10' : undefined,
    );
    await useSelectionToolbarStore.getState().executeTool(translateTool);
    expect(invokeMock).toHaveBeenCalledWith('selection_toolbar_execute_tool', {
      selectionId: 'selection-1',
      toolId: 'translate',
      options: { source_language: 'en', target_language: 'ja' },
    });
  });

  it('sends no language options for non-translate tools', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'selection_toolbar_get_snapshot') {
        return {
          runtime: {
            state: 'running',
            platform: 'macos',
            permission: 'granted',
            last_error: null,
            global_dismissal_supported: true,
          },
          session: {
            selection_id: 'selection-1',
            tools: [],
            theme: 'light',
            language: 'en-US',
          },
          run: null,
        };
      }
      if (command === 'selection_toolbar_execute_tool') return 'request-11';
      return undefined;
    });
    const { useSelectionToolbarStore } = await import('../selectionToolbarStore');
    await useSelectionToolbarStore.getState().initialize();

    await useSelectionToolbarStore.getState().executeTool({
      id: 'summarize',
      kind: 'ai',
      icon: 'list-collapse',
      builtin_key: 'summarize',
      name: null,
    });

    expect(invokeMock).toHaveBeenCalledWith('selection_toolbar_execute_tool', {
      selectionId: 'selection-1',
      toolId: 'summarize',
      options: null,
    });
  });

  it('routes stop, result copy, and close through request and selection identifiers', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'selection_toolbar_get_snapshot') {
        return {
          runtime: {
            state: 'running',
            platform: 'macos',
            permission: 'granted',
            last_error: null,
            global_dismissal_supported: true,
          },
          session: {
            selection_id: 'selection-1',
            tools: [],
            theme: 'light',
            language: 'en-US',
          },
          run: {
            request_id: 'request-1',
            selection_id: 'selection-1',
            tool_id: 'summarize',
            status: 'stopped',
            output: 'partial',
            error: null,
          },
        };
      }
      return undefined;
    });
    const { useSelectionToolbarStore } = await import('../selectionToolbarStore');
    await useSelectionToolbarStore.getState().initialize();

    await useSelectionToolbarStore.getState().stop();
    await useSelectionToolbarStore.getState().copyResult();
    await useSelectionToolbarStore.getState().close('close_button');

    expect(invokeMock).toHaveBeenCalledWith('selection_toolbar_stop_generation', {
      requestId: 'request-1',
    });
    expect(invokeMock).toHaveBeenCalledWith('selection_toolbar_copy_result', {
      requestId: 'request-1',
    });
    expect(invokeMock).toHaveBeenCalledWith('selection_toolbar_close', {
      reason: 'close_button',
    });
    expect(useSelectionToolbarStore.getState().session).toBeNull();
  });
});
