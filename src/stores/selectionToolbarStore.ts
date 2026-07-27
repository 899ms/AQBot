import { create } from 'zustand';
import { invoke, listen, type UnlistenFn } from '@/lib/invoke';
import type {
  SelectionToolbarRunEvent,
  SelectionToolbarRunView,
  SelectionToolbarRuntimeStatus,
  SelectionToolbarSessionView,
  SelectionToolbarSnapshot,
  SelectionToolbarToolView,
} from '@/types';

const EMPTY_RUNTIME: SelectionToolbarRuntimeStatus = {
  state: 'disabled',
  platform: 'unsupported',
  permission: 'unknown',
  last_error: null,
  global_dismissal_supported: false,
};

let initialization: Promise<void> | null = null;
let unlisteners: UnlistenFn[] = [];
let eventRevision = 0;
let copyCloseTimer: number | null = null;

function cancelCopyCloseTimer() {
  if (copyCloseTimer !== null) {
    window.clearTimeout(copyCloseTimer);
    copyCloseTimer = null;
  }
}

export interface SelectionToolbarTranslateOptions {
  sourceLanguage?: string | null;
  targetLanguage?: string | null;
}

interface SelectionToolbarState {
  runtime: SelectionToolbarRuntimeStatus;
  session: SelectionToolbarSessionView | null;
  run: SelectionToolbarRunView | null;
  surface: 'toolbar' | 'overflow' | 'result';
  copied: boolean;
  busy: boolean;
  error: string | null;
  /** Translate panel source language; 'auto' means auto-detect. */
  translateSource: string;
  /** Translate panel target language; null falls back to the configured/app language. */
  translateTarget: string | null;
  initialize: () => Promise<void>;
  executeTool: (
    tool: SelectionToolbarToolView,
    options?: SelectionToolbarTranslateOptions,
  ) => Promise<void>;
  setTranslateLanguages: (source: string, target: string) => Promise<void>;
  stop: () => Promise<void>;
  copyResult: () => Promise<void>;
  regenerate: () => Promise<void>;
  close: (reason: string) => Promise<void>;
  toggleOverflow: () => Promise<void>;
  dispose: () => void;
}

function isTranslateTool(tool: SelectionToolbarToolView): boolean {
  return tool.kind === 'ai' && tool.builtin_key === 'translate';
}

function applyRunEvent(
  state: SelectionToolbarState,
  event: SelectionToolbarRunEvent,
): Partial<SelectionToolbarState> {
  if (state.session?.selection_id !== event.selection_id) return {};
  if (event.kind === 'started') {
    return {
      run: {
        request_id: event.request_id,
        selection_id: event.selection_id,
        tool_id: event.tool_id,
        status: 'started',
        output: '',
        error: null,
      },
      surface: 'result',
      error: null,
    };
  }
  if (!state.run || state.run.request_id !== event.request_id) return {};
  if (event.kind === 'delta') {
    return {
      run: {
        ...state.run,
        status: 'streaming',
        output: state.run.output + event.delta,
      },
    };
  }
  if (event.kind === 'error') {
    return {
      run: { ...state.run, status: 'error', error: event.error },
      error: event.error,
    };
  }
  return {
    run: {
      ...state.run,
      status: event.kind === 'completed' ? 'completed' : 'stopped',
      // Terminal events may carry the think-tag-finalized output.
      output: event.output ?? state.run.output,
    },
  };
}

export const useSelectionToolbarStore = create<SelectionToolbarState>((set, get) => ({
  runtime: EMPTY_RUNTIME,
  session: null,
  run: null,
  surface: 'toolbar',
  copied: false,
  busy: false,
  error: null,
  translateSource: 'auto',
  translateTarget: null,

  initialize: async () => {
    if (initialization) return initialization;
    initialization = (async () => {
      unlisteners = await Promise.all([
        listen<SelectionToolbarSessionView>('selection-toolbar://session', ({ payload }) => {
          eventRevision += 1;
          document.documentElement.dataset.theme = payload.theme;
          document.documentElement.lang = payload.language;
          set((state) =>
            state.session?.selection_id === payload.selection_id
              ? { session: payload, busy: false }
              : {
                  session: payload,
                  run: null,
                  surface: 'toolbar',
                  copied: false,
                  busy: false,
                  error: null,
                  translateSource: 'auto',
                  translateTarget: null,
                },
          );
        }),
        listen<string>('selection-toolbar://hidden', () => {
          eventRevision += 1;
          cancelCopyCloseTimer();
          set({
            session: null,
            run: null,
            surface: 'toolbar',
            copied: false,
            busy: false,
            error: null,
            translateSource: 'auto',
            translateTarget: null,
          });
        }),
        listen<SelectionToolbarRunEvent>('selection-toolbar://run', ({ payload }) => {
          eventRevision += 1;
          set((state) => applyRunEvent(state, payload));
        }),
      ]);
      const revisionBeforeSnapshot = eventRevision;
      const snapshot = await invoke<SelectionToolbarSnapshot>('selection_toolbar_get_snapshot');
      if (eventRevision === revisionBeforeSnapshot) {
        set({
          runtime: snapshot.runtime,
          session: snapshot.session,
          run: snapshot.run,
          surface: snapshot.run ? 'result' : 'toolbar',
          busy: false,
          error: snapshot.run?.error ?? null,
        });
      } else {
        set({ runtime: snapshot.runtime });
      }
      // Tell the backend listeners are live so any pending session is flushed.
      try {
        await invoke('selection_toolbar_frontend_ready');
      } catch {
        // Non-fatal in browser mock / partial capability.
      }
    })().catch((error) => {
      initialization = null;
      set({ error: String(error), busy: false });
      throw error;
    });
    return initialization;
  },

  executeTool: async (tool, options) => {
    if (get().busy) return;
    const session = get().session;
    if (!session) {
      set({ error: 'Selection is no longer active' });
      return;
    }
    // Running another tool must cancel a pending copy-close so the result
    // panel is not torn down ~700ms later.
    cancelCopyCloseTimer();
    set({ busy: true, error: null });
    try {
      if (tool.kind === 'action') {
        await invoke('selection_toolbar_copy_selection', {
          selectionId: session.selection_id,
        });
        set({ copied: true, busy: false });
        copyCloseTimer = window.setTimeout(() => {
          copyCloseTimer = null;
          // Only auto-close if no AI run took over in the meantime.
          if (!get().run) {
            void get().close('copy_completed');
          }
        }, 700);
        return;
      }
      // The translate tool always runs with the panel's language choices so
      // re-clicks and regenerate keep the user's selection.
      const effective = options
        ?? (isTranslateTool(tool)
          ? {
              sourceLanguage: get().translateSource,
              targetLanguage: get().translateTarget,
            }
          : undefined);
      const requestId = await invoke<string>('selection_toolbar_execute_tool', {
        selectionId: session.selection_id,
        toolId: tool.id,
        options: effective
          ? {
              source_language: effective.sourceLanguage ?? null,
              target_language: effective.targetLanguage ?? null,
            }
          : null,
      });
      if (!get().run || get().run?.request_id !== requestId) {
        set({
          run: {
            request_id: requestId,
            selection_id: session.selection_id,
            tool_id: tool.id,
            status: 'started',
            output: '',
            error: null,
          },
          surface: 'result',
        });
      }
      set({ busy: false });
    } catch (error) {
      const message = String(error);
      set({
        run: {
          request_id: `frontend-error-${Date.now()}`,
          selection_id: session.selection_id,
          tool_id: tool.id,
          status: 'error',
          output: '',
          error: message,
        },
        surface: 'result',
        error: message,
        busy: false,
      });
      try {
        await invoke('selection_toolbar_set_surface', { surface: 'result' });
      } catch (surfaceError) {
        const combined = `${message}\n${String(surfaceError)}`;
        set((state) => ({
          error: combined,
          run: state.run ? { ...state.run, error: combined } : state.run,
        }));
      }
    }
  },

  setTranslateLanguages: async (source, target) => {
    const previousTarget = get().translateTarget;
    set({ translateSource: source, translateTarget: target });
    if (target !== previousTarget) {
      // Persist so future sessions open with the chosen target; a failure only
      // affects the default of later sessions, not this run.
      void invoke('selection_toolbar_set_translate_target', { language: target }).catch(
        (error) => {
          console.warn('Failed to persist translate target language:', error);
        },
      );
    }
    const tool = get().session?.tools.find(isTranslateTool);
    if (!tool) return;
    await get().executeTool(tool, { sourceLanguage: source, targetLanguage: target });
  },

  stop: async () => {
    const run = get().run;
    if (!run) return;
    await invoke('selection_toolbar_stop_generation', { requestId: run.request_id });
  },

  copyResult: async () => {
    const run = get().run;
    if (!run) return;
    await invoke('selection_toolbar_copy_result', { requestId: run.request_id });
    set({ copied: true });
    window.setTimeout(() => set({ copied: false }), 700);
  },

  regenerate: async () => {
    const { run, session, busy } = get();
    if (!run || !session || busy) return;
    if (run.status === 'started' || run.status === 'streaming') return;
    const tool = session.tools.find((candidate) => candidate.id === run.tool_id);
    if (!tool) return;
    await get().executeTool(tool);
  },

  close: async (reason) => {
    cancelCopyCloseTimer();
    await invoke('selection_toolbar_close', { reason });
    set({
      session: null,
      run: null,
      surface: 'toolbar',
      copied: false,
      busy: false,
      error: null,
      translateSource: 'auto',
      translateTarget: null,
    });
  },

  toggleOverflow: async () => {
    if (get().busy) return;
    const surface = get().surface === 'overflow' ? 'toolbar' : 'overflow';
    set({ surface });
    await invoke('selection_toolbar_set_surface', { surface });
  },

  dispose: () => {
    unlisteners.forEach((unlisten) => unlisten());
    unlisteners = [];
    eventRevision = 0;
    initialization = null;
  },
}));
