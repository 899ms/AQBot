export const SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS = 5;

export const SELECTION_TOOLBAR_CUSTOM_ICONS = [
  'wand-sparkles',
  'languages',
  'spell-check',
  'list-collapse',
  'brain',
  'book-open',
  'code',
  'message-square',
  'pen-line',
  'search',
  'sparkles',
  'terminal',
] as const;

export type SelectionToolbarCustomIcon = typeof SELECTION_TOOLBAR_CUSTOM_ICONS[number];
export type SelectionToolbarBuiltinAiKey = 'translate' | 'polish' | 'summarize';

export interface SelectionToolbarAiConfig {
  prompt: string;
  provider_id: string | null;
  model_id: string | null;
  temperature: number | null;
  top_p: number | null;
  max_tokens: number | null;
}

export type SelectionToolbarTool =
  | {
      kind: 'builtin_ai';
      builtin_key: SelectionToolbarBuiltinAiKey;
      enabled: boolean;
      ai: SelectionToolbarAiConfig;
    }
  | {
      kind: 'builtin_action';
      builtin_key: 'copy';
      enabled: boolean;
    }
  | {
      kind: 'custom_ai';
      id: string;
      name: string;
      icon: SelectionToolbarCustomIcon;
      enabled: boolean;
      ai: SelectionToolbarAiConfig;
    };

export interface SelectionToolbarSettings {
  enabled: boolean;
  theme_follow: boolean;
  tools: SelectionToolbarTool[];
}

function ai(prompt: string): SelectionToolbarAiConfig {
  return {
    prompt,
    provider_id: null,
    model_id: null,
    temperature: null,
    top_p: null,
    max_tokens: null,
  };
}

export function createDefaultSelectionToolbarSettings(): SelectionToolbarSettings {
  return {
    enabled: false,
    theme_follow: false,
    tools: [
      {
        kind: 'builtin_ai',
        builtin_key: 'translate',
        enabled: true,
        ai: ai('Translate the following text into the current application language. Return only the translation:\n\n{selection}'),
      },
      {
        kind: 'builtin_ai',
        builtin_key: 'polish',
        enabled: true,
        ai: ai('Polish the following text while preserving its meaning. Return only the polished text:\n\n{selection}'),
      },
      {
        kind: 'builtin_ai',
        builtin_key: 'summarize',
        enabled: true,
        ai: ai('Summarize the following text concisely in the current application language:\n\n{selection}'),
      },
      {
        kind: 'builtin_action',
        builtin_key: 'copy',
        enabled: true,
      },
    ],
  };
}

export type SelectionToolbarRuntimeState =
  | 'disabled'
  | 'starting'
  | 'running'
  | 'permission_required'
  | 'unavailable'
  | 'error';

export interface SelectionToolbarRuntimeStatus {
  state: SelectionToolbarRuntimeState;
  platform: 'macos' | 'windows' | 'linux' | 'unsupported';
  permission: 'not_required' | 'granted' | 'denied' | 'unknown';
  last_error: { code: string; message: string } | null;
  global_dismissal_supported: boolean;
}

export type SelectionToolbarPermissionSettingsOutcome =
  | { kind: 'prompt_requested' }
  | { kind: 'permission_pane_opened' }
  | { kind: 'manual_add_required'; executable_path: string };

export interface SelectionToolbarToolView {
  id: string;
  kind: 'ai' | 'action';
  builtin_key: SelectionToolbarBuiltinAiKey | 'copy' | null;
  name: string | null;
  icon: string;
}

export interface SelectionToolbarSessionView {
  selection_id: string;
  tools: SelectionToolbarToolView[];
  theme: 'light' | 'dark';
  language: string;
}

export interface SelectionToolbarRunView {
  request_id: string;
  selection_id: string;
  tool_id: string;
  status: 'started' | 'streaming' | 'completed' | 'stopped' | 'error';
  output: string;
  error: string | null;
}

export interface SelectionToolbarSnapshot {
  runtime: SelectionToolbarRuntimeStatus;
  session: SelectionToolbarSessionView | null;
  run: SelectionToolbarRunView | null;
}

export type SelectionToolbarRunEvent =
  | { kind: 'started'; request_id: string; selection_id: string; tool_id: string }
  | { kind: 'delta'; request_id: string; selection_id: string; delta: string }
  | { kind: 'completed'; request_id: string; selection_id: string; output?: string | null }
  | { kind: 'stopped'; request_id: string; selection_id: string; output?: string | null }
  | { kind: 'error'; request_id: string; selection_id: string; error: string };
