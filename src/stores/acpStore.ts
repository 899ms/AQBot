import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { invoke, listen, type UnlistenFn } from '@/lib/invoke';
import type {
  AcpAgentsFile,
  AcpMessage,
  AcpPromptAccepted,
  AcpProject,
  AcpRecentThreadReceipt,
  AcpSessionSnapshot,
  AcpThread,
  ConfiguredAgent,
  RegistryFile,
} from '@/types/acp';
import type { PermissionOptionButton } from '@/components/chat/PermissionCard';
import type { AttachmentInput } from '@/types';

// Generation counter prevents StrictMode / remount races from stacking
// multiple acp-stream-text listeners (which doubles every streamed character).
let _acpListenerGen = 0;
let _acpUnlisten: UnlistenFn | null = null;
let _acpEventsReady: Promise<UnlistenFn> | null = null;
let _acpBootstrapInFlight: Promise<void> | null = null;
let _acpPrewarmInFlight: Promise<void> | null = null;
const _acpPrepareInFlight = new Map<string, Promise<AcpSessionSnapshot>>();
const _acpMessageLoadVersion = new Map<string, number>();
const _acpFirstOutputTimers = new Map<string, ReturnType<typeof setTimeout>>();
let _acpOptimisticMessageSeq = 0;
let _acpInteractionSeq = 0;

const FIRST_OUTPUT_SILENCE_MS = 12_000;
export const ACP_HOST_STATUS = {
  firstOutputSilence: 'aqbot:first-output-silence',
  cancelling: 'aqbot:cancelling',
  cancelRestarting: 'aqbot:cancel-restarting',
  usingSharedAgent: 'aqbot:using-shared-agent',
  launchingAgent: 'aqbot:launching-agent',
  agentReady: 'aqbot:agent-ready',
  restoringSession: 'aqbot:restoring-session',
  savedSessionExpired: 'aqbot:saved-session-expired',
  creatingSession: 'aqbot:creating-session',
  sendingPrompt: 'aqbot:sending-prompt',
  sessionExpired: 'aqbot:session-expired',
  grokRetry: 'aqbot:grok-retry:',
} as const;
export const ACP_STATUS_FIRST_OUTPUT_SILENCE = ACP_HOST_STATUS.firstOutputSilence;
export const ACP_STATUS_CANCELLING = ACP_HOST_STATUS.cancelling;

const REPLACEABLE_FIRST_OUTPUT_STATUSES = new Set<string>([
  ACP_HOST_STATUS.sendingPrompt,
  ACP_HOST_STATUS.agentReady,
  ACP_HOST_STATUS.usingSharedAgent,
  ACP_HOST_STATUS.launchingAgent,
]);

function canReplaceWithFirstOutputSilence(status: string | undefined): boolean {
  return !status?.trim() || REPLACEABLE_FIRST_OUTPUT_STATUSES.has(status);
}

function clearFirstOutputTimer(threadId: string): void {
  const timer = _acpFirstOutputTimers.get(threadId);
  if (timer) clearTimeout(timer);
  _acpFirstOutputTimers.delete(threadId);
}

interface AcpPrewarmResult {
  agentId: string;
  ready: boolean;
  error?: string | null;
}

export interface AcpAgentReadiness {
  status: 'ready' | 'error';
  error: string | null;
}

function snapshotCurrentMode(snapshot: AcpSessionSnapshot): string | null {
  if (snapshot.modes?.currentModeId) return snapshot.modes.currentModeId;
  const configMode = snapshot.configOptions.find((option) => {
    if (option.type !== 'select' || !Array.isArray(option.options)) return false;
    return option.options.some((entry) => (
      'value' in entry
        ? String(entry.value).split(/[#/:]/).pop()?.toLowerCase() === 'plan'
        : entry.options.some(
            (choice) => String(choice.value).split(/[#/:]/).pop()?.toLowerCase() === 'plan',
          )
    ));
  });
  return typeof configMode?.currentValue === 'string' ? configMode.currentValue : null;
}

function launchFingerprint(agent: ConfiguredAgent | undefined): string | null {
  if (!agent) return null;
  return JSON.stringify([agent.command, agent.args, agent.env ?? {}]);
}

function ensureAcpEventsBound(bind: () => Promise<UnlistenFn>): Promise<UnlistenFn> {
  if (_acpEventsReady) return _acpEventsReady;
  const pending = bind();
  _acpEventsReady = pending.catch((error) => {
    _acpEventsReady = null;
    throw error;
  });
  return _acpEventsReady;
}

export interface AcpPlanEntry {
  content: string;
  status: string;
  priority?: string;
}

export interface AcpPlanState {
  entries: AcpPlanEntry[];
  completed: number;
  total: number;
}

/** Persisted plan-review document shown in the conversation timeline after exit. */
export interface AcpPlanDocument {
  id: string;
  threadId: string;
  messageId?: string;
  content: string;
  title?: string;
  status: 'pending' | 'approved' | 'cancelled' | 'abandoned' | 'expired';
  sequence: number;
  createdAt: string;
  feedback?: string;
}

function extractPlanDocumentContent(
  input: Record<string, unknown> | null | undefined,
  extras?: { description?: string | null; title?: string | null },
): string {
  const source = input ?? {};
  const candidates = [
    extras?.description,
    source.planContent,
    source.plan_content,
    source.content,
    source.description,
    extras?.title,
    source.title,
  ];
  for (const value of candidates) {
    if (typeof value === 'string' && value.trim()) return value;
  }
  return '';
}

function planDocumentStatusFromResolution(
  optionId: string | undefined,
  reason: 'selected' | 'cancelled' | 'expired' | undefined,
): AcpPlanDocument['status'] {
  if (reason === 'expired') return 'expired';
  if (reason === 'cancelled') return 'expired';
  const id = String(optionId ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
  if (id === 'approved' || id === 'approve') return 'approved';
  if (id === 'cancelled' || id === 'cancel') return 'cancelled';
  if (id === 'abandoned' || id === 'abandon') return 'abandoned';
  if (reason === 'selected') return 'approved';
  return 'expired';
}

function upsertPlanDocument(
  byThread: Record<string, AcpPlanDocument[]>,
  document: AcpPlanDocument,
): Record<string, AcpPlanDocument[]> {
  const existing = byThread[document.threadId] ?? [];
  const index = existing.findIndex((item) => item.id === document.id);
  const nextList = index >= 0
    ? existing.map((item, i) => (i === index
      ? {
          ...item,
          ...document,
          // Keep the earliest sequence/createdAt so timeline order is stable.
          sequence: item.sequence,
          createdAt: item.createdAt,
          content: document.content || item.content,
          title: document.title ?? item.title,
          messageId: document.messageId ?? item.messageId,
          feedback: document.feedback ?? item.feedback,
        }
      : item))
    : [...existing, document];
  return { ...byThread, [document.threadId]: nextList };
}

function resolvePlanDocument(
  byThread: Record<string, AcpPlanDocument[]>,
  requestId: string,
  patch: Partial<Pick<AcpPlanDocument, 'status' | 'feedback' | 'messageId' | 'content' | 'title'>>,
): Record<string, AcpPlanDocument[]> {
  let changed = false;
  const next: Record<string, AcpPlanDocument[]> = {};
  for (const [threadId, docs] of Object.entries(byThread)) {
    next[threadId] = docs.map((doc) => {
      if (doc.id !== requestId) return doc;
      changed = true;
      return {
        ...doc,
        ...patch,
        content: patch.content || doc.content,
        title: patch.title ?? doc.title,
        messageId: patch.messageId ?? doc.messageId,
        feedback: patch.feedback ?? doc.feedback,
      };
    });
  }
  return changed ? next : byThread;
}

/** When a turn ends, keep plan bodies but mark still-pending reviews as expired. */
function finalizePendingPlanDocuments(
  byThread: Record<string, AcpPlanDocument[]>,
  threadId: string,
): Record<string, AcpPlanDocument[]> {
  const docs = byThread[threadId];
  if (!docs?.some((doc) => doc.status === 'pending')) return byThread;
  return {
    ...byThread,
    [threadId]: docs.map((doc) => (
      doc.status === 'pending' ? { ...doc, status: 'expired' as const } : doc
    )),
  };
}

/**
 * Session plan progress (todo checklist). Only structured ACP plan entries
 * count — never markdown `planContent` from plan-review documents, which used
 * to produce garbage todos like form field labels.
 *
 * Returns `null` when the payload is not a real progress update so callers can
 * leave the existing checklist alone.
 */
function normalizePlan(raw: Record<string, unknown>): AcpPlanState | null {
  const kind = String(raw.kind ?? raw.sessionUpdate ?? raw.session_update ?? '')
    .toLowerCase()
    .replace(/[^a-z0-9_]/g, '');
  // Plan-review documents and exit-plan-mode payloads are not progress lists.
  if (kind === 'plan_review' || kind === 'planreview' || kind.includes('planreview')) {
    return null;
  }
  // Document-only payloads (planContent without structured entries).
  const hasStructuredEntries = Array.isArray(raw.entries)
    || Array.isArray((raw.plan as Record<string, unknown> | undefined)?.entries);
  if (!hasStructuredEntries) {
    return null;
  }

  const source = Array.isArray(raw.entries)
    ? raw.entries
    : Array.isArray((raw.plan as Record<string, unknown> | undefined)?.entries)
      ? ((raw.plan as Record<string, unknown>).entries as unknown[])
      : [];

  const entries = source
    .map((item) => {
      const entry = (item ?? {}) as Record<string, unknown>;
      const content = String(entry.content ?? entry.title ?? entry.description ?? '').trim();
      if (!content) return null;
      return {
        content,
        status: String(entry.status ?? 'pending').toLowerCase(),
        ...(entry.priority ? { priority: String(entry.priority) } : {}),
      };
    })
    .filter((entry): entry is AcpPlanEntry => entry != null);

  // Empty structured array is a valid "clear progress" signal from the agent.
  const completed = entries.filter((entry) =>
    ['completed', 'complete', 'done'].includes(entry.status),
  ).length;
  return { entries, completed, total: entries.length };
}

function decodeXmlTextEntities(value: string): string {
  return value
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');
}

function parseHtmlAttrMap(openTag: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  const re = /([:@A-Za-z_][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(openTag)) != null) {
    const key = match[1].toLowerCase();
    const value = match[2] ?? match[3] ?? match[4] ?? '';
    attrs[key] = decodeXmlTextEntities(value);
  }
  return attrs;
}

function normalizePlanDocumentStatus(value: string | undefined): AcpPlanDocument['status'] {
  const id = String(value ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
  if (id === 'approved' || id === 'approve') return 'approved';
  if (id === 'cancelled' || id === 'cancel') return 'cancelled';
  if (id === 'abandoned' || id === 'abandon') return 'abandoned';
  if (id === 'expired') return 'expired';
  if (id === 'pending') return 'pending';
  // Markers that survived a completed turn without a status default to approved
  // so the card remains readable after reload.
  return 'approved';
}

/**
 * Rebuild plan-review documents from inline `<acp-plan>` markers in message
 * content (the durable source of truth after a refresh).
 */
function persistedPlanDocuments(messages: AcpMessage[]): Record<string, AcpPlanDocument[]> {
  const byThread: Record<string, AcpPlanDocument[]> = {};
  const re = /<acp-plan\b([^>]*)>([\s\S]*?)<\/acp-plan>/gi;
  for (const message of messages) {
    if (message.role !== 'assistant' || !message.content) continue;
    let match: RegExpExecArray | null;
    re.lastIndex = 0;
    let sequence = 0;
    while ((match = re.exec(message.content)) != null) {
      const attrs = parseHtmlAttrMap(match[1] ?? '');
      if (attrs['data-aqbot'] !== '1' && attrs['data-aqbot'] !== 'true') {
        // Still accept markers we emit (always have data-aqbot="1"), but be lenient.
      }
      const id = attrs.id?.trim();
      if (!id) continue;
      const body = decodeXmlTextEntities(match[2] ?? '').trim();
      const title = attrs.title?.trim() || undefined;
      const content = body || title || '';
      if (!content) continue;
      const document: AcpPlanDocument = {
        id,
        threadId: message.thread_id,
        messageId: attrs.message?.trim() || message.id,
        content,
        title,
        status: normalizePlanDocumentStatus(attrs.status),
        sequence: sequence++,
        createdAt: message.created_at,
      };
      const list = byThread[document.threadId] ?? [];
      const index = list.findIndex((item) => item.id === document.id);
      if (index >= 0) {
        list[index] = {
          ...list[index],
          ...document,
          // Prefer longer content if a later message revisits the same id.
          content: document.content.length >= list[index].content.length
            ? document.content
            : list[index].content,
        };
      } else {
        list.push(document);
      }
      byThread[document.threadId] = list;
    }
  }
  return byThread;
}

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
  kind?: 'permission' | 'question' | 'plan_review';
  title?: string;
  toolName: string;
  toolCallId?: string;
  input: Record<string, unknown>;
  options: Array<PermissionOptionButton & { kind?: string; description?: string }>;
  status: 'pending' | 'approved' | 'denied';
  messageId?: string;
  sequence?: number;
}

export type AcpQuestionnaireOutcome =
  | 'accepted'
  | 'chat_about_this'
  | 'skip_interview'
  | 'cancelled';

export interface AcpQuestionnaireAnswer {
  questionIndex: number;
  selectedOptionIndexes: number[];
  otherText?: string;
}

export interface AcpQuestionnaireSubmission {
  outcome: AcpQuestionnaireOutcome;
  answers: AcpQuestionnaireAnswer[];
}

export interface AcpToolCallState {
  threadId: string;
  messageId?: string;
  toolCallId: string;
  toolName: string;
  status: 'queued' | 'running' | 'success' | 'error' | 'cancelled';
  input?: string;
  output?: string;
  approvalStatus?: 'approved' | 'denied' | 'cancelled' | 'expired';
  approvalOptionId?: string;
  approvalLabel?: string;
}

interface AcpInteractionResolution {
  requestId: string;
  threadId?: string;
  messageId?: string;
  kind?: AcpPermissionRequest['kind'];
  toolCallId?: string;
  optionId?: string;
  optionKind?: string;
  optionLabel?: string;
  reason?: 'selected' | 'cancelled' | 'expired';
}

function resolvedInteractionState(
  pendingPermissions: Record<string, AcpPermissionRequest>,
  toolCalls: Record<string, AcpToolCallState>,
  resolution: AcpInteractionResolution,
): {
  pendingPermissions: Record<string, AcpPermissionRequest>;
  toolCalls: Record<string, AcpToolCallState>;
} {
  const existing = pendingPermissions[resolution.requestId];
  const { [resolution.requestId]: _resolved, ...remaining } = pendingPermissions;
  const selectedOption = existing?.options.find((option) => option.id === resolution.optionId);
  const kind = resolution.kind ?? existing?.kind ?? 'permission';
  const threadId = resolution.threadId ?? existing?.threadId;
  const toolCallId = resolution.toolCallId ?? existing?.toolCallId;
  if (!threadId || !toolCallId) {
    return { pendingPermissions: remaining, toolCalls };
  }

  const messageId = resolution.messageId ?? existing?.messageId;
  const toolKey = acpToolStateKey(threadId, toolCallId, messageId);
  const previousTool = toolCalls[toolKey];
  const selectedLabel = resolution.optionLabel?.trim()
    || selectedOption?.label.trim()
    || undefined;
  const previousOutput = previousTool?.output?.trim()
    ? previousTool.output
    : undefined;
  const questionnaireOutcome = resolution.optionId
    ? `aqbot:questionnaire:${resolution.optionId}`
    : undefined;
  const baseTool: AcpToolCallState = {
    threadId,
    messageId: messageId ?? previousTool?.messageId,
    toolCallId,
    toolName: previousTool?.toolName ?? existing?.toolName ?? 'tool',
    status: previousTool?.status ?? 'queued',
    input: previousTool?.input
      ?? (existing ? JSON.stringify(existing.input, null, 2) : undefined),
    output: previousTool?.output,
  };
  if (kind !== 'permission') {
    return {
      pendingPermissions: remaining,
      toolCalls: {
        ...toolCalls,
        [toolKey]: {
          ...baseTool,
          output: resolution.reason === 'selected'
            ? previousOutput ?? selectedLabel ?? questionnaireOutcome
            : previousTool?.output,
        },
      },
    };
  }

  const decisionIdentity = `${resolution.optionKind ?? selectedOption?.kind ?? ''} ${
    resolution.optionId ?? ''
  }`.toLowerCase();
  const denied = selectedOption?.variant === 'danger'
    || /reject|deny|cancel|abandon/.test(decisionIdentity);
  const approvalStatus: AcpToolCallState['approvalStatus'] = resolution.reason === 'cancelled'
    ? 'cancelled'
    : resolution.reason === 'expired'
      ? 'expired'
      : denied
        ? 'denied'
        : 'approved';
  const approvalLabel = resolution.optionLabel ?? selectedOption?.label;
  return {
    pendingPermissions: remaining,
    toolCalls: {
      ...toolCalls,
      [toolKey]: {
        ...baseTool,
        status: approvalStatus === 'approved' ? baseTool.status : 'cancelled',
        approvalStatus,
        ...(resolution.optionId ? { approvalOptionId: resolution.optionId } : {}),
        ...(approvalLabel ? { approvalLabel } : {}),
      },
    },
  };
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
  turnActivityByThread: Record<string, boolean>;
  pendingPermissions: Record<string, AcpPermissionRequest>; // requestId
  toolCalls: Record<string, AcpToolCallState>; // threadId:messageId:toolCallId
  agentReadinessById: Record<string, AcpAgentReadiness>;
  sessionByThread: Record<string, AcpSessionSnapshot>;
  preparingByThread: Record<string, boolean>;
  cancellingByThread: Record<string, boolean>;
  planByThread: Record<string, AcpPlanState>;
  /** Full plan-review documents kept for timeline re-reading after exit. */
  planDocumentsByThread: Record<string, AcpPlanDocument[]>;
  /**
   * Thread-scoped tools the user chose "始终允许" for — auto-approve matching
   * permission prompts for the rest of this AQBot thread/session.
   */
  alwaysAllowedToolsByThread: Record<string, string[]>;
  spawnModelByThread: Record<string, string>;
  spawnReasoningByThread: Record<string, string>;
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
  createRecentThread: (agentId: string, title?: string) => Promise<AcpThread>;
  deleteThread: (threadId: string) => Promise<void>;
  renameThread: (threadId: string, title: string) => Promise<AcpThread>;
  toggleThreadPin: (threadId: string) => Promise<AcpThread>;
  /** Optimistic local reorder within a project (sidebar drag). */
  setThreadsOrder: (projectId: string, threads: AcpThread[]) => void;
  reorderThreads: (projectId: string, threadIds: string[]) => Promise<void>;
  duplicateThread: (threadId: string, titleSuffix?: string) => Promise<AcpThread>;
  selectThread: (threadId: string | null) => Promise<void>;
  loadMessages: (threadId: string) => Promise<void>;
  prepareDraft: (projectId: string, agentId: string) => Promise<AcpSessionSnapshot>;
  prepareSession: (threadId: string) => Promise<AcpSessionSnapshot>;
  setConfigOption: (
    threadId: string,
    configId: string,
    value: string | boolean,
  ) => Promise<void>;
  setSessionMode: (threadId: string, modeId: string) => Promise<void>;
  cancelPrompt: (threadId: string) => Promise<void>;
  sendPrompt: (
    threadId: string,
    prompt: string,
    attachments?: AttachmentInput[],
  ) => Promise<void>;
  respondPermission: (
    requestId: string,
    optionId: string,
    feedback?: string,
  ) => Promise<void>;
  respondQuestionnaire: (
    requestId: string,
    submission: AcpQuestionnaireSubmission,
  ) => Promise<void>;
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
  options: Array<{
    optionId?: string;
    option_id?: string;
    name: string;
    kind?: string | null;
    description?: string | null;
  }>,
  descriptions: Array<string | undefined> = [],
): Array<PermissionOptionButton & { kind?: string; description?: string }> {
  if (!options?.length) return [];
  return options.map((o, index) => {
    const id = o.optionId ?? o.option_id ?? o.name;
    const kind = (o.kind ?? '').toLowerCase();
    let variant: PermissionOptionButton['variant'] = 'default';
    if (kind.includes('reject') || kind.includes('deny') || kind.includes('cancel')) {
      variant = 'danger';
    } else if (kind.includes('allow_once') || kind.includes('allowonce')) {
      variant = 'primary';
    } else if (kind.includes('allow_always') || kind.includes('allowalways')) {
      variant = 'default';
    } else if (kind.includes('allow')) {
      variant = 'primary';
    }
    return {
      id,
      label: o.name || id,
      variant,
      ...(o.kind ? { kind: o.kind } : {}),
      ...(o.description || descriptions[index]
        ? { description: o.description ?? descriptions[index] ?? undefined }
        : {}),
    };
  });
}

/** Normalize tool name for session always-allow matching. */
export function acpPermissionToolKey(toolName: string | null | undefined): string {
  return String(toolName ?? '').trim().toLowerCase();
}

const ACP_SESSION_ALWAYS_ALLOW_OPTION_ID = '__aqbot_session_always_allow';

function optionIdentity(option: { id?: string; kind?: string | null }): string {
  return `${String(option.id ?? '').toLowerCase().replace(/[^a-z0-9]/g, '')} ${
    String(option.kind ?? '').toLowerCase().replace(/[^a-z0-9]/g, '')
  }`.trim();
}

function isSessionAlwaysAllowOption(option: { id?: string; kind?: string | null }): boolean {
  const identity = optionIdentity(option);
  return identity.includes('allowalways') || option.id === ACP_SESSION_ALWAYS_ALLOW_OPTION_ID;
}

function findAgentAllowOptionId(
  options: Array<{ id: string; kind?: string }>,
): string | undefined {
  const realAlways = options.find(
    (option) => isSessionAlwaysAllowOption(option)
      && option.id !== ACP_SESSION_ALWAYS_ALLOW_OPTION_ID,
  );
  if (realAlways) return realAlways.id;
  const once = options.find((option) => {
    const identity = optionIdentity(option);
    return identity.includes('allowonce')
      || identity === 'allow'
      || identity === 'approved'
      || identity === 'approve';
  });
  if (once) return once.id;
  return options.find((option) => {
    const identity = optionIdentity(option);
    return identity.includes('allow')
      && !identity.includes('reject')
      && !identity.includes('deny');
  })?.id;
}

function rememberAlwaysAllowedTool(
  byThread: Record<string, string[]>,
  threadId: string,
  toolName: string,
): Record<string, string[]> {
  const key = acpPermissionToolKey(toolName);
  if (!key) return byThread;
  const existing = byThread[threadId] ?? [];
  if (existing.includes(key)) return byThread;
  return { ...byThread, [threadId]: [...existing, key] };
}

function isToolAlwaysAllowed(
  byThread: Record<string, string[]>,
  threadId: string,
  toolName: string,
): boolean {
  const key = acpPermissionToolKey(toolName);
  if (!key) return false;
  return (byThread[threadId] ?? []).includes(key);
}

function interactionKind(raw: Record<string, unknown>): AcpPermissionRequest['kind'] {
  const kind = String(raw.kind ?? '').toLowerCase();
  if (kind === 'ask_user_question') return 'question';
  if (kind === 'plan_review') return 'plan_review';
  return 'permission';
}

function removeThreadEntries<T extends { threadId: string }>(
  entries: Record<string, T>,
  threadId: string,
): Record<string, T> {
  return Object.fromEntries(
    Object.entries(entries).filter(([, entry]) => entry.threadId !== threadId),
  );
}

export function acpToolStateKey(
  threadId: string,
  toolCallId: string,
  messageId?: string,
): string {
  return messageId
    ? `${threadId}:${messageId}:${toolCallId}`
    : `${threadId}:${toolCallId}`;
}

function prewarmConfiguredAgents(): Promise<void> {
  // ACP processes only exist in the desktop runtime. Browser preview/tests use
  // BrowserMock, which intentionally does not emulate long-lived child processes.
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    return Promise.resolve();
  }
  if (_acpPrewarmInFlight) return _acpPrewarmInFlight;

  const task = invoke<AcpPrewarmResult[]>(
    'acp_prewarm_enabled_agents',
  )
    .then((results) => {
      const updates = Object.fromEntries(results.map((result) => [
        result.agentId,
        {
          status: result.ready ? 'ready' as const : 'error' as const,
          error: result.error ?? null,
        },
      ]));
      useAcpStore.setState((state) => ({
        agentReadinessById: { ...state.agentReadinessById, ...updates },
      }));
      const failed = results.filter((result) => !result.ready);
      if (failed.length > 0) console.warn('ACP startup prewarm failed', failed);
    })
    .catch((error) => {
      const message = String(error);
      useAcpStore.setState((state) => ({
        agentReadinessById: {
          ...state.agentReadinessById,
          ...Object.fromEntries(
            (state.config?.agents ?? [])
              .filter((agent) => agent.enabled)
              .map((agent) => [agent.id, { status: 'error' as const, error: message }]),
          ),
        },
      }));
      console.warn('ACP startup prewarm command failed', error);
    })
    .finally(() => {
      if (_acpPrewarmInFlight === task) _acpPrewarmInFlight = null;
    });
  _acpPrewarmInFlight = task;
  return task;
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

function extractToolOutput(raw: Record<string, unknown>): string | undefined {
  const value = raw.rawOutput ?? raw.raw_output ?? raw.output ?? raw.content;
  if (value == null) return undefined;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function normalizeToolStatus(value: unknown): AcpToolCallState['status'] {
  const status = String(value ?? 'success').toLowerCase();
  if (status === 'completed' || status === 'success') return 'success';
  if (status === 'failed' || status === 'error') return 'error';
  if (status === 'cancelled') return 'cancelled';
  if (status === 'in_progress' || status === 'running') return 'running';
  return 'queued';
}

function normalizeApprovalStatus(value: unknown): AcpToolCallState['approvalStatus'] {
  if (value === 'approved' || value === 'denied' || value === 'cancelled' || value === 'expired') {
    return value;
  }
  return undefined;
}

function persistedToolCalls(messages: AcpMessage[]): Record<string, AcpToolCallState> {
  const toolCalls: Record<string, AcpToolCallState> = {};
  for (const message of messages) {
    if (!message.meta_json) continue;
    let parsed: unknown;
    try {
      parsed = JSON.parse(message.meta_json);
    } catch (error) {
      console.warn('[acpStore] invalid ACP message metadata', { messageId: message.id, error });
      continue;
    }
    const rawTools = (parsed as { toolCalls?: unknown })?.toolCalls;
    if (!Array.isArray(rawTools)) continue;
    for (const raw of rawTools) {
      if (!raw || typeof raw !== 'object') continue;
      const tool = raw as Record<string, unknown>;
      const toolCallId = typeof tool.toolCallId === 'string' ? tool.toolCallId : '';
      if (!toolCallId) continue;
      const approvalStatus = normalizeApprovalStatus(tool.approvalStatus);
      toolCalls[acpToolStateKey(message.thread_id, toolCallId, message.id)] = {
        threadId: message.thread_id,
        messageId: message.id,
        toolCallId,
        toolName: typeof tool.toolName === 'string' && tool.toolName ? tool.toolName : 'tool',
        status: normalizeToolStatus(tool.status),
        ...(typeof tool.input === 'string' ? { input: tool.input } : {}),
        ...(typeof tool.output === 'string' ? { output: tool.output } : {}),
        ...(approvalStatus ? { approvalStatus } : {}),
        ...(typeof tool.approvalOptionId === 'string'
          ? { approvalOptionId: tool.approvalOptionId }
          : {}),
        ...(typeof tool.approvalLabel === 'string' ? { approvalLabel: tool.approvalLabel } : {}),
      };
    }
  }
  return toolCalls;
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
  turnActivityByThread: {},
  pendingPermissions: {},
  toolCalls: {},
  agentReadinessById: {},
  sessionByThread: {},
  preparingByThread: {},
  cancellingByThread: {},
  planByThread: {},
  planDocumentsByThread: {},
  alwaysAllowedToolsByThread: {},
  spawnModelByThread: {},
  spawnReasoningByThread: {},
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
    Object.values(get().pendingPermissions)
      .filter((permission) => permission.threadId === threadId)
      .sort((left, right) => (
        (left.sequence ?? Number.MAX_SAFE_INTEGER)
        - (right.sequence ?? Number.MAX_SAFE_INTEGER)
      )),

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
      if (refresh) {
        const previousConfig = get().config;
        const config = await invoke<AcpAgentsFile>('acp_get_config');
        const previousById = new Map(
          (previousConfig?.agents ?? []).map((agent) => [agent.id, launchFingerprint(agent)]),
        );
        const changedAgents = new Set(
          config.agents
            .filter((agent) => previousById.get(agent.id) !== launchFingerprint(agent))
            .map((agent) => agent.id),
        );
        set((state) => {
          const threadAgent = new Map(
            [...state.allThreads, ...state.threads].map((thread) => [thread.id, thread.agent_id]),
          );
          const keepSession = ([key]: [string, unknown]) => {
            const agentId = key.startsWith('draft:')
              ? key.split(':').slice(-1)[0]
              : threadAgent.get(key);
            return !agentId || !changedAgents.has(agentId) || state.runningByThread[key] === true;
          };
          return {
            registry,
            config,
            permissionMode: mapPermissionDefaultToMode(config.general?.permissionDefault),
            loading: false,
            sessionByThread: Object.fromEntries(
              Object.entries(state.sessionByThread).filter(keepSession),
            ),
            spawnModelByThread: Object.fromEntries(
              Object.entries(state.spawnModelByThread).filter(keepSession),
            ),
            spawnReasoningByThread: Object.fromEntries(
              Object.entries(state.spawnReasoningByThread).filter(keepSession),
            ),
          };
        });
        if (changedAgents.size > 0) {
          void prewarmConfiguredAgents();
        }
      } else {
        set({ registry, loading: false });
      }
    } catch (e) {
      const refreshError = String(e);
      set({ loading: false, error: refreshError });
      try {
        const registry = await invoke<RegistryFile>('acp_get_registry');
        set({ registry });
      } catch (fallbackError) {
        set({
          error: `${refreshError}; failed to load cached ACP Registry: ${String(fallbackError)}`,
        });
      }
    }
  },

  setAgentEnabled: async (agentId, enabled) => {
    const config = await invoke<AcpAgentsFile>('acp_set_agent_enabled', { agentId, enabled });
    set({ config });
    prewarmConfiguredAgents();
  },

  addFromRegistry: async (agentId) => {
    const config = await invoke<AcpAgentsFile>('acp_add_agent_from_registry', {
      agentId,
      enabled: true,
    });
    set({ config });
    prewarmConfiguredAgents();
  },

  saveGeneral: async (general) => {
    const config = await invoke<AcpAgentsFile>('acp_save_general', { general });
    set({
      config,
      permissionMode: mapPermissionDefaultToMode(config.general?.permissionDefault),
    });
    prewarmConfiguredAgents();
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
    prewarmConfiguredAgents();
  },

  removeAgent: async (agentId) => {
    const config = await invoke<AcpAgentsFile>('acp_remove_agent', { agentId });
    set({ config });
    prewarmConfiguredAgents();
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
    if (_acpBootstrapInFlight) return;
    // Revalidate cached state once per renderer lifetime. Process prewarm is
    // global; conversation sessions remain lazy and are prepared on selection.
    const desktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
    const loading = Promise.all([
      get().loadConfig(),
      get().loadProjects(),
      get().loadAllThreads(),
    ]).then(async () => {
      if (!desktop) return;
      // Start from the validated local configuration immediately. Registry
      // refresh is optional network I/O (up to its request timeout) and must
      // never delay Agent readiness at application startup.
      await prewarmConfiguredAgents();
      if (get().config?.general.registryRefresh === 'on_start') {
        await get().loadRegistry(true);
      }
    });
    _acpBootstrapInFlight = loading;
    if (desktop) {
      void ensureAcpEventsBound(get().bindEvents).catch((error) => {
        console.warn('Failed to bind ACP event listeners', error);
      });
    }
  },

  createThread: async (projectId, agentId, title) => {
    const draftKey = `draft:${projectId}:${agentId}`;
    const thread = await invoke<AcpThread>('acp_create_thread', {
      projectId,
      agentId,
      title: title ?? null,
    });
    set((state) => {
      const draftSnapshot = state.sessionByThread[draftKey];
      const { [draftKey]: _adoptedDraft, ...remainingSessions } = state.sessionByThread;
      const { [draftKey]: draftReasoning, ...remainingReasoning } =
        state.spawnReasoningByThread;
      const { [draftKey]: draftModel, ...remainingModels } = state.spawnModelByThread;
      return {
        activeProjectId: projectId,
        activeThreadId: thread.id,
        threads: [
          thread,
          ...state.threads.filter((item) => (
            item.id !== thread.id && item.project_id === projectId
          )),
        ],
        allThreads: [thread, ...state.allThreads.filter((item) => item.id !== thread.id)],
        messages: [],
        sessionByThread: {
          ...remainingSessions,
          ...(draftSnapshot ? { [thread.id]: draftSnapshot } : {}),
        },
        spawnReasoningByThread: {
          ...remainingReasoning,
          ...(draftReasoning ? { [thread.id]: draftReasoning } : {}),
        },
        spawnModelByThread: {
          ...remainingModels,
          ...(draftModel ? { [thread.id]: draftModel } : {}),
        },
      };
    });
    if (!get().sessionByThread[thread.id]) {
      void get().prepareSession(thread.id).catch(() => undefined);
    }
    // The create receipt is authoritative for the first turn. Sidebar caches
    // reconcile in the background and must not delay prompt scheduling.
    void Promise.all([get().loadThreads(projectId), get().loadAllThreads()]).catch((error) => {
      set({ error: String(error) });
    });
    return thread;
  },

  createRecentThread: async (agentId, title) => {
    const { project, thread } = await invoke<AcpRecentThreadReceipt>('acp_create_recent_thread', {
      agentId,
      title: title ?? null,
    });
    set((state) => ({
      projects: [
        ...state.projects.filter((item) => item.id !== project.id),
        project,
      ],
      activeProjectId: thread.project_id,
      activeThreadId: thread.id,
      threads: [thread],
      allThreads: [thread, ...state.allThreads.filter((item) => item.id !== thread.id)],
      messages: [],
    }));
    void get().prepareSession(thread.id).catch(() => undefined);
    return thread;
  },

  deleteThread: async (threadId) => {
    const stateBeforeDelete = get();
    const threadBeforeDelete = [...stateBeforeDelete.threads, ...stateBeforeDelete.allThreads]
      .find((thread) => thread.id === threadId);
    const recentProjectId = stateBeforeDelete.projects.find(
      (project) => project.id === threadBeforeDelete?.project_id && project.kind === 'recent',
    )?.id;
    await invoke('acp_delete_thread', { threadId });
    const { activeProjectId, activeThreadId } = get();
    set((state) => {
      const { [threadId]: _removedReasoning, ...spawnReasoningByThread } =
        state.spawnReasoningByThread;
      const { [threadId]: _removedModel, ...spawnModelByThread } = state.spawnModelByThread;
      const { [threadId]: _removedSession, ...sessionByThread } = state.sessionByThread;
      const { [threadId]: _removedAlways, ...alwaysAllowedToolsByThread } =
        state.alwaysAllowedToolsByThread;
      const { [threadId]: _removedPlans, ...planDocumentsByThread } =
        state.planDocumentsByThread;
      return {
        spawnModelByThread,
        spawnReasoningByThread,
        sessionByThread,
        alwaysAllowedToolsByThread,
        planDocumentsByThread,
        pendingPermissions: removeThreadEntries(state.pendingPermissions, threadId),
        toolCalls: removeThreadEntries(state.toolCalls, threadId),
        ...(recentProjectId
          ? { projects: state.projects.filter((project) => project.id !== recentProjectId) }
          : {}),
        ...(activeThreadId === threadId ? { activeThreadId: null, messages: [] } : {}),
        ...(recentProjectId && activeProjectId === recentProjectId
          ? { activeProjectId: null, threads: [] }
          : {}),
      };
    });
    if (activeProjectId && activeProjectId !== recentProjectId) {
      await get().loadThreads(activeProjectId);
    }
    await get().loadAllThreads();
  },

  renameThread: async (threadId, title) => {
    const thread = await invoke<AcpThread>('acp_rename_thread', { threadId, title });
    const patch = (list: AcpThread[]) =>
      list.map((th) => (th.id === threadId ? { ...th, ...thread } : th));
    set((s) => ({
      threads: patch(s.threads),
      allThreads: patch(s.allThreads),
    }));
    return thread;
  },

  toggleThreadPin: async (threadId) => {
    const thread = await invoke<AcpThread>('acp_toggle_thread_pin', { threadId });
    // Reload to re-apply pin/sort ordering from backend
    await get().loadAllThreads();
    const { activeProjectId } = get();
    if (activeProjectId) await get().loadThreads(activeProjectId);
    return thread;
  },

  setThreadsOrder: (projectId, ordered) => {
    set((s) => {
      const others = s.allThreads.filter((th) => th.project_id !== projectId);
      const allThreads = [...others, ...ordered];
      return {
        allThreads,
        ...(s.activeProjectId === projectId ? { threads: ordered } : {}),
      };
    });
  },

  reorderThreads: async (projectId, threadIds) => {
    await invoke('acp_reorder_threads', { projectId, threadIds });
    set((s) => {
      const byId = new Map(
        [...s.threads, ...s.allThreads]
          .filter((th) => th.project_id === projectId)
          .map((th) => [th.id, th]),
      );
      const ordered = threadIds
        .map((id, i) => {
          const th = byId.get(id);
          return th ? { ...th, sort_order: i } : null;
        })
        .filter(Boolean) as AcpThread[];
      const others = s.allThreads.filter((th) => th.project_id !== projectId);
      return {
        allThreads: [...others, ...ordered],
        ...(s.activeProjectId === projectId ? { threads: ordered } : {}),
      };
    });
  },

  duplicateThread: async (threadId, titleSuffix) => {
    const thread = await invoke<AcpThread>('acp_duplicate_thread', {
      threadId,
      titleSuffix: titleSuffix ?? null,
    });
    await get().loadAllThreads();
    const { activeProjectId } = get();
    if (activeProjectId === thread.project_id) {
      await get().loadThreads(thread.project_id);
    }
    return thread;
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
    if (!get().sessionByThread[threadId]) {
      void get().prepareSession(threadId).catch(() => undefined);
    }
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
    const version = (_acpMessageLoadVersion.get(threadId) ?? 0) + 1;
    _acpMessageLoadVersion.set(threadId, version);
    const messages = await invoke<AcpMessage[]>('acp_list_messages', { threadId });
    if (_acpMessageLoadVersion.get(threadId) !== version) return;
    // If a turn is still running, preserve local streaming content so a mid-turn
    // reload does not wipe the live buffer or revive a stuck spinner.
    set((s) => {
      if (s.activeThreadId !== threadId) return s;
      const hydratedTools = persistedToolCalls(messages);
      const toolCallsForOtherThreads = removeThreadEntries(s.toolCalls, threadId);
      const hydratedPlans = persistedPlanDocuments(messages);
      const hydratedThreadPlans = hydratedPlans[threadId] ?? [];
      // Keep live in-memory plan docs (pending reviews / richer content) while
      // still filling gaps from durable markers after a refresh.
      const livePlans = s.planDocumentsByThread[threadId] ?? [];
      const planById = new Map<string, AcpPlanDocument>();
      for (const doc of hydratedThreadPlans) planById.set(doc.id, doc);
      for (const doc of livePlans) {
        const existing = planById.get(doc.id);
        if (!existing) {
          planById.set(doc.id, doc);
          continue;
        }
        // Live session state wins on conflict (status / feedback); prefer the
        // longer plan body so neither side loses content on reload.
        planById.set(doc.id, {
          ...existing,
          ...doc,
          content: (doc.content?.length ?? 0) >= (existing.content?.length ?? 0)
            ? doc.content
            : existing.content,
          title: doc.title ?? existing.title,
          messageId: doc.messageId ?? existing.messageId,
          feedback: doc.feedback ?? existing.feedback,
          sequence: Math.min(existing.sequence, doc.sequence),
          createdAt: existing.createdAt || doc.createdAt,
        });
      }
      const mergedPlans = [...planById.values()].sort((a, b) => a.sequence - b.sequence);
      const planDocumentsByThread = {
        ...s.planDocumentsByThread,
        [threadId]: mergedPlans,
      };

      if (!s.runningByThread[threadId]) {
        return {
          messages,
          toolCalls: { ...toolCallsForOtherThreads, ...hydratedTools },
          planDocumentsByThread,
        };
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
      return {
        messages: merged,
        toolCalls: { ...toolCallsForOtherThreads, ...hydratedTools, ...s.toolCalls },
        planDocumentsByThread,
      };
    });
  },

  prepareDraft: async (projectId, agentId) => {
    const key = `draft:${projectId}:${agentId}`;
    const existing = _acpPrepareInFlight.get(key);
    if (existing) return existing;
    set((s) => ({
      preparingByThread: { ...s.preparingByThread, [key]: true },
    }));
    const task = invoke<AcpSessionSnapshot>('acp_prepare_draft', {
      projectId,
      agentId,
      modelId: get().spawnModelByThread[key] ?? null,
      reasoningEffort: get().spawnReasoningByThread[key] ?? null,
    })
      .then((snapshot) => {
        set((s) => ({
          sessionByThread: { ...s.sessionByThread, [key]: snapshot },
        }));
        return snapshot;
      })
      .catch((error) => {
        set((s) => ({
          statusByThread: { ...s.statusByThread, [key]: String(error) },
          error: String(error),
        }));
        throw error;
      })
      .finally(() => {
        _acpPrepareInFlight.delete(key);
        set((s) => ({
          preparingByThread: { ...s.preparingByThread, [key]: false },
        }));
      });
    _acpPrepareInFlight.set(key, task);
    return task;
  },

  prepareSession: async (threadId) => {
    const existing = _acpPrepareInFlight.get(threadId);
    if (existing) return existing;
    set((s) => ({
      preparingByThread: { ...s.preparingByThread, [threadId]: true },
    }));
    const task = invoke<AcpSessionSnapshot>('acp_prepare_session', {
      threadId,
      modelId: get().spawnModelByThread[threadId] ?? null,
      reasoningEffort: get().spawnReasoningByThread[threadId] ?? null,
    })
      .then((snapshot) => {
        set((s) => ({
          sessionByThread: { ...s.sessionByThread, [threadId]: snapshot },
          statusByThread: { ...s.statusByThread, [threadId]: '' },
          threads: s.threads.map((thread) => (
            thread.id === threadId
              ? { ...thread, mode_id: snapshotCurrentMode(snapshot) }
              : thread
          )),
          allThreads: s.allThreads.map((thread) => (
            thread.id === threadId
              ? { ...thread, mode_id: snapshotCurrentMode(snapshot) }
              : thread
          )),
        }));
        return snapshot;
      })
      .catch((error) => {
        set((s) => ({
          statusByThread: { ...s.statusByThread, [threadId]: String(error) },
          error: String(error),
        }));
        throw error;
      })
      .finally(() => {
        _acpPrepareInFlight.delete(threadId);
        set((s) => ({
          preparingByThread: { ...s.preparingByThread, [threadId]: false },
        }));
      });
    _acpPrepareInFlight.set(threadId, task);
    return task;
  },

  setConfigOption: async (threadId, configId, value) => {
    const before = get().sessionByThread[threadId]?.configOptions.find(
      (option) => option.id === configId,
    );
    const snapshot = await invoke<AcpSessionSnapshot>('acp_set_config_option', {
      threadId,
      configId,
      value,
    });
    const after = snapshot.configOptions.find((option) => option.id === configId);
    const spawnArg = after?._meta?.aqbotSpawnArg;
    const category = after?.category ?? before?.category;
    const isModelControl = category === 'model'
      || before?._meta?.aqbotSpawnArg === '--model'
      || spawnArg === '--model';
    const isReasoningControl = category === 'thought_level'
      || /reasoning|effort/i.test(configId)
      || before?._meta?.aqbotSpawnArg === '--reasoning-effort'
      || spawnArg === '--reasoning-effort';
    set((s) => ({
      sessionByThread: { ...s.sessionByThread, [threadId]: snapshot },
      threads: s.threads.map((thread) => (
        thread.id === threadId
          ? { ...thread, mode_id: snapshotCurrentMode(snapshot) }
          : thread
      )),
      allThreads: s.allThreads.map((thread) => (
        thread.id === threadId
          ? { ...thread, mode_id: snapshotCurrentMode(snapshot) }
          : thread
      )),
      ...(isModelControl && typeof value === 'string'
        ? {
            spawnModelByThread: spawnArg !== '--model' || value === '__agent_default'
              ? Object.fromEntries(
                  Object.entries(s.spawnModelByThread).filter(([key]) => key !== threadId),
                )
              : { ...s.spawnModelByThread, [threadId]: value },
          }
        : {}),
      ...(isReasoningControl && typeof value === 'string'
        ? {
            spawnReasoningByThread: spawnArg !== '--reasoning-effort'
              || value === '__agent_default'
              ? Object.fromEntries(
                  Object.entries(s.spawnReasoningByThread).filter(([key]) => key !== threadId),
                )
              : { ...s.spawnReasoningByThread, [threadId]: value },
          }
        : {}),
    }));
  },

  setSessionMode: async (threadId, modeId) => {
    const snapshot = await invoke<AcpSessionSnapshot>('acp_set_mode', { threadId, modeId });
    set((s) => {
      const syncMode = (thread: AcpThread) =>
        thread.id === threadId ? { ...thread, mode_id: modeId } : thread;
      return {
        sessionByThread: { ...s.sessionByThread, [threadId]: snapshot },
        threads: s.threads.map(syncMode),
        allThreads: s.allThreads.map(syncMode),
      };
    });
  },

  cancelPrompt: async (threadId) => {
    clearFirstOutputTimer(threadId);
    set((s) => ({
      cancellingByThread: { ...s.cancellingByThread, [threadId]: true },
      turnActivityByThread: { ...s.turnActivityByThread, [threadId]: true },
      statusByThread: { ...s.statusByThread, [threadId]: ACP_STATUS_CANCELLING },
    }));
    try {
      const cancelled = await invoke<boolean>('acp_cancel', { threadId });
      if (!cancelled) throw new Error('No active ACP turn to cancel');
      await get().loadMessages(threadId);
      const stillStreaming = get().messages.some(
        (message) => message.thread_id === threadId && message.status === 'streaming',
      );
      if (!stillStreaming) {
        set((s) => ({
          runningByThread: { ...s.runningByThread, [threadId]: false },
          cancellingByThread: { ...s.cancellingByThread, [threadId]: false },
          statusByThread: { ...s.statusByThread, [threadId]: '' },
          planDocumentsByThread: finalizePendingPlanDocuments(
            s.planDocumentsByThread,
            threadId,
          ),
          pendingPermissions: removeThreadEntries(s.pendingPermissions, threadId),
        }));
      }
    } catch (error) {
      set((s) => ({
        cancellingByThread: { ...s.cancellingByThread, [threadId]: false },
      }));
      throw error;
    }
  },

  sendPrompt: async (threadId, prompt, attachments) => {
    // Invalidate any list request started before this turn. Its snapshot cannot
    // contain the rows created below and must never erase them when it resolves.
    _acpMessageLoadVersion.set(threadId, (_acpMessageLoadVersion.get(threadId) ?? 0) + 1);
    const optimisticSequence = ++_acpOptimisticMessageSeq;
    const optimisticUserId = `optimistic-user:${threadId}:${optimisticSequence}`;
    const optimisticAssistantId = `optimistic-assistant:${threadId}:${optimisticSequence}`;
    const optimisticCreatedAt = new Date().toISOString();
    clearFirstOutputTimer(threadId);
    set((s) => ({
      runningByThread: { ...s.runningByThread, [threadId]: true },
      turnActivityByThread: { ...s.turnActivityByThread, [threadId]: false },
      statusByThread: { ...s.statusByThread, [threadId]: '' },
      planDocumentsByThread: finalizePendingPlanDocuments(
        s.planDocumentsByThread,
        threadId,
      ),
      pendingPermissions: removeThreadEntries(s.pendingPermissions, threadId),
      error: null,
      messages: s.activeThreadId === threadId
        ? [
            ...s.messages,
            {
              id: optimisticUserId,
              thread_id: threadId,
              role: 'user',
              content: prompt,
              status: 'done',
              attachments: [],
              created_at: optimisticCreatedAt,
            },
            {
              id: optimisticAssistantId,
              thread_id: threadId,
              role: 'assistant',
              content: '',
              status: 'streaming',
              attachments: [],
              created_at: optimisticCreatedAt,
            },
          ]
        : s.messages,
    }));
    const firstOutputTimer = setTimeout(() => {
      _acpFirstOutputTimers.delete(threadId);
      const state = get();
      const lastAssistant = [...state.messages]
        .reverse()
        .find((message) => message.thread_id === threadId && message.role === 'assistant');
      const hasBody = !!lastAssistant?.content?.trim();
      const hasPermission = Object.values(state.pendingPermissions).some(
        (permission) => permission.threadId === threadId && permission.status === 'pending',
      );
      if (
        !state.runningByThread[threadId]
        || state.turnActivityByThread[threadId]
        || hasBody
        || hasPermission
        || !canReplaceWithFirstOutputSilence(state.statusByThread[threadId])
      ) return;
      set((current) => ({
        statusByThread: {
          ...current.statusByThread,
          [threadId]: ACP_STATUS_FIRST_OUTPUT_SILENCE,
        },
      }));
    }, FIRST_OUTPUT_SILENCE_MS);
    _acpFirstOutputTimers.set(threadId, firstOutputTimer);
    try {
      // A terminal event must never be missed, even on a very fast first turn.
      await ensureAcpEventsBound(get().bindEvents);
      const accepted = await invoke<AcpPromptAccepted>('acp_prompt', {
        threadId,
        prompt,
        attachments: attachments ?? null,
        modelId: get().spawnModelByThread[threadId] ?? null,
        reasoningEffort: get().spawnReasoningByThread[threadId] ?? null,
      });
      _acpMessageLoadVersion.set(threadId, (_acpMessageLoadVersion.get(threadId) ?? 0) + 1);
      // The command returns the exact persisted rows. Installing this receipt is
      // atomic and avoids a second DB read racing an older empty list request.
      set((s) => {
        if (s.activeThreadId !== threadId) return s;
        const existingAssistant = s.messages.find(
          (message) => message.id === accepted.assistantMessage.id,
        );
        const assistantMessage = existingAssistant
          ? {
              ...accepted.assistantMessage,
              content: existingAssistant.content || accepted.assistantMessage.content,
              status: existingAssistant.status ?? accepted.assistantMessage.status,
              meta_json: existingAssistant.meta_json ?? accepted.assistantMessage.meta_json,
            }
          : accepted.assistantMessage;
        const acceptedIds = new Set([
          accepted.userMessage.id,
          accepted.assistantMessage.id,
          optimisticUserId,
          optimisticAssistantId,
        ]);
        return {
          messages: [
            ...s.messages.filter((message) => !acceptedIds.has(message.id)),
            accepted.userMessage,
            assistantMessage,
          ],
        };
      });
    } catch (e) {
      clearFirstOutputTimer(threadId);
      set((s) => {
        const optimisticIds = new Set([optimisticUserId, optimisticAssistantId]);
        return {
          runningByThread: { ...s.runningByThread, [threadId]: false },
          turnActivityByThread: { ...s.turnActivityByThread, [threadId]: true },
          messages: s.messages.filter((message) => !optimisticIds.has(message.id)),
          error: String(e),
        };
      });
      throw e;
    }
  },

  respondPermission: async (requestId, optionId, feedback) => {
    const trimmedFeedback = feedback?.trim();
    const existing = get().pendingPermissions[requestId];
    let resolvedOptionId = optionId;
    let rememberAlways = false;

    if (existing && (existing.kind ?? 'permission') === 'permission') {
      const selected = existing.options.find((option) => option.id === optionId);
      const syntheticAlways = optionId === ACP_SESSION_ALWAYS_ALLOW_OPTION_ID
        || isSessionAlwaysAllowOption({ id: optionId, kind: selected?.kind });
      if (syntheticAlways) {
        rememberAlways = true;
        if (optionId === ACP_SESSION_ALWAYS_ALLOW_OPTION_ID) {
          const agentAllowId = findAgentAllowOptionId(existing.options);
          if (!agentAllowId) {
            throw new Error('No allow option available for session always-allow');
          }
          resolvedOptionId = agentAllowId;
        }
      }
    }

    await invoke('acp_respond_permission', {
      requestId,
      optionId: resolvedOptionId,
      feedback: trimmedFeedback || null,
    });
    set((s) => {
      const current = s.pendingPermissions[requestId] ?? existing;
      if (!current) {
        if (!rememberAlways || !existing) return s;
        return {
          alwaysAllowedToolsByThread: rememberAlwaysAllowedTool(
            s.alwaysAllowedToolsByThread,
            existing.threadId,
            existing.toolName,
          ),
        };
      }
      const selectedOption = current.options.find((option) => option.id === resolvedOptionId)
        ?? current.options.find((option) => option.id === optionId);
      const next = resolvedInteractionState(s.pendingPermissions, s.toolCalls, {
        requestId,
        reason: 'selected',
        optionId: resolvedOptionId,
        optionKind: rememberAlways
          ? 'AllowAlways'
          : selectedOption?.kind,
        optionLabel: rememberAlways
          ? (selectedOption?.label ?? '始终允许')
          : selectedOption?.label,
      });
      const withAlways = rememberAlways
        ? {
            ...next,
            alwaysAllowedToolsByThread: rememberAlwaysAllowedTool(
              s.alwaysAllowedToolsByThread,
              current.threadId,
              current.toolName,
            ),
          }
        : next;
      if (current.kind !== 'plan_review') return withAlways;
      return {
        ...withAlways,
        planDocumentsByThread: resolvePlanDocument(s.planDocumentsByThread, requestId, {
          status: planDocumentStatusFromResolution(resolvedOptionId, 'selected'),
          messageId: current.messageId,
          feedback: trimmedFeedback || undefined,
        }),
      };
    });
  },

  respondQuestionnaire: async (requestId, submission) => {
    const summary = await invoke<string>('acp_respond_questionnaire', {
      requestId,
      outcome: submission.outcome,
      answers: submission.answers,
    });
    set((s) => resolvedInteractionState(s.pendingPermissions, s.toolCalls, {
      requestId,
      reason: submission.outcome === 'cancelled' ? 'cancelled' : 'selected',
      optionId: submission.outcome,
      optionLabel: summary || undefined,
    }));
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
    const markTurnActivity = (threadId: string) => {
      clearFirstOutputTimer(threadId);
      set((state) => ({
        turnActivityByThread: { ...state.turnActivityByThread, [threadId]: true },
        ...(state.statusByThread[threadId] === ACP_STATUS_FIRST_OUTPUT_SILENCE
          ? { statusByThread: { ...state.statusByThread, [threadId]: '' } }
          : {}),
      }));
    };
    const streamBatch = new Map<string, { threadId: string; text: string }>();
    let streamTimer: ReturnType<typeof setTimeout> | null = null;
    const flushStreams = () => {
      if (streamTimer) clearTimeout(streamTimer);
      streamTimer = null;
      if (!streamBatch.size || !isLive()) return;
      const batch = [...streamBatch.entries()];
      streamBatch.clear();
      set((s) => {
        const streamingText = { ...s.streamingText };
        const runningByThread = { ...s.runningByThread };
        let messages = s.messages;
        for (const [messageId, pending] of batch) {
          const existing = messages.find((message) => message.id === messageId);
          if (
            runningByThread[pending.threadId] === false
            && existing
            && (existing.status === 'done' || existing.status === 'error')
          ) {
            continue;
          }
          const nextStream = mergeStreamChunk(streamingText[messageId] ?? '', pending.text);
          streamingText[messageId] = nextStream;
          runningByThread[pending.threadId] = true;
          if (existing) {
            messages = messages.map((message) =>
              message.id === messageId
                ? { ...message, content: nextStream, status: 'streaming' }
                : message,
            );
          } else if (s.activeThreadId === pending.threadId) {
            messages = [
              ...messages,
              {
                id: messageId,
                thread_id: pending.threadId,
                role: 'assistant',
                content: nextStream,
                status: 'streaming',
                attachments: [],
                created_at: new Date().toISOString(),
              },
            ];
          }
        }
        return { streamingText, runningByThread, messages };
      });
    };
    const queueStream = (threadId: string, messageId: string, text: string) => {
      const pending = streamBatch.get(messageId);
      streamBatch.set(messageId, {
        threadId,
        text: mergeStreamChunk(pending?.text ?? '', text),
      });
      if (!streamTimer) streamTimer = setTimeout(flushStreams, 16);
    };

    unlisteners.push(
      await listen<{ threadId: string; messageId: string; text: string }>(
        'acp-stream-text',
        (event) => {
          if (!isLive()) return;
          const { threadId, messageId, text } = event.payload;
          markTurnActivity(threadId);
          queueStream(threadId, messageId, text ?? '');
        },
      ),
    );

    unlisteners.push(
      await listen<{ threadId: string; snapshot: AcpSessionSnapshot }>(
        'acp-session-state',
        (event) => {
          if (!isLive()) return;
          const { threadId, snapshot } = event.payload;
          const modeId = snapshotCurrentMode(snapshot);
          set((s) => ({
            sessionByThread: { ...s.sessionByThread, [threadId]: snapshot },
            threads: s.threads.map((thread) => (
              thread.id === threadId ? { ...thread, mode_id: modeId } : thread
            )),
            allThreads: s.allThreads.map((thread) => (
              thread.id === threadId ? { ...thread, mode_id: modeId } : thread
            )),
          }));
        },
      ),
    );

    unlisteners.push(
      await listen<{ threadId: string; messageId?: string; raw: Record<string, unknown> }>(
        'acp-plan',
        (event) => {
          if (!isLive()) return;
          const { threadId, raw } = event.payload;
          const plan = normalizePlan(raw ?? {});
          // Ignore plan-review documents / non-structured payloads so they
          // never overwrite the real session todo checklist.
          if (!plan) return;
          markTurnActivity(threadId);
          set((s) => ({
            planByThread: { ...s.planByThread, [threadId]: plan },
          }));
        },
      ),
    );

    unlisteners.push(
      await listen<{ threadId: string; message: string; preparing?: boolean }>('acp-status', (event) => {
        if (!isLive()) return;
        const { threadId, message, preparing } = event.payload;
        set((s) => ({
          statusByThread: {
            ...s.statusByThread,
            [threadId]: message,
          },
          ...(!preparing
            ? { runningByThread: { ...s.runningByThread, [threadId]: true } }
            : {}),
        }));
      }),
    );

    unlisteners.push(
      await listen<{
        threadId: string;
        messageId?: string;
        requestId: string;
        interactionKind?: AcpPermissionRequest['kind'];
        toolCallId?: string | null;
        title?: string | null;
        raw: Record<string, unknown>;
        options: Array<{
          optionId?: string;
          option_id?: string;
          name: string;
          kind?: string;
          description?: string | null;
        }>;
      }>('acp-permission-request', (event) => {
        if (!isLive()) return;
        const {
          threadId,
          messageId,
          requestId,
          interactionKind: eventInteractionKind,
          toolCallId: eventToolCallId,
          title: eventTitle,
          raw,
          options,
        } = event.payload;
        markTurnActivity(threadId);
        const toolCall = (raw.toolCall ?? raw.tool_call ?? raw) as Record<string, unknown>;
        const kind = eventInteractionKind ?? interactionKind(raw);
        const toolCallId = eventToolCallId
          ?? (typeof toolCall.toolCallId === 'string' ? toolCall.toolCallId : null)
          ?? (typeof toolCall.tool_call_id === 'string' ? toolCall.tool_call_id : undefined);
        const toolName =
          (typeof toolCall.kind === 'string' && toolCall.kind)
          || (typeof toolCall.toolName === 'string' && toolCall.toolName)
          || (typeof toolCall.title === 'string' && String(toolCall.title).slice(0, 40))
          || 'tool';
        const inputObj =
          (toolCall.rawInput as Record<string, unknown>)
          || (toolCall.input as Record<string, unknown>)
          || toolCall;
        const title = eventTitle
          ?? (typeof raw.title === 'string' ? raw.title : undefined)
          ?? (typeof toolCall.title === 'string' ? toolCall.title : undefined);
        const input = typeof inputObj === 'object' && inputObj ? inputObj : { value: inputObj };
        const mappedOptions = mapAcpOptions(options ?? []);
        const sequence = ++_acpInteractionSeq;

        // Session always-allow: auto-approve without surfacing the composer.
        if (
          kind === 'permission'
          && isToolAlwaysAllowed(get().alwaysAllowedToolsByThread, threadId, String(toolName))
        ) {
          const allowOptionId = findAgentAllowOptionId(mappedOptions);
          if (allowOptionId) {
            void invoke('acp_respond_permission', {
              requestId,
              optionId: allowOptionId,
              feedback: null,
            }).catch((error) => {
              console.error('[acp] session always-allow auto-respond failed', error);
            });
            return;
          }
        }

        set((s) => {
          const nextPermissions = {
            ...s.pendingPermissions,
            [requestId]: {
              threadId,
              messageId,
              requestId,
              kind,
              title,
              toolName: String(toolName),
              toolCallId,
              input,
              options: mappedOptions,
              status: 'pending' as const,
              sequence,
            },
          };
          if (kind !== 'plan_review') {
            return { pendingPermissions: nextPermissions };
          }
          const content = extractPlanDocumentContent(input, {
            description: typeof raw.description === 'string' ? raw.description : undefined,
            title,
          });
          return {
            pendingPermissions: nextPermissions,
            planDocumentsByThread: upsertPlanDocument(s.planDocumentsByThread, {
              id: requestId,
              threadId,
              messageId,
              content,
              title: typeof title === 'string' ? title : undefined,
              status: 'pending',
              sequence,
              createdAt: new Date().toISOString(),
            }),
          };
        });
      }),
    );

    unlisteners.push(
      await listen<{
        threadId: string;
        messageId?: string;
        requestId: string;
        interactionKind?: AcpPermissionRequest['kind'];
        toolCallId?: string | null;
        reason: 'selected' | 'cancelled' | 'expired';
        selectedOptionId?: string | null;
        selectedOptionKind?: string | null;
        selectedOptionName?: string | null;
      }>('acp-interaction-closed', (event) => {
        if (!isLive()) return;
        const payload = event.payload;
        markTurnActivity(payload.threadId);
        set((state) => {
          const resolution = {
            requestId: payload.requestId,
            reason: payload.reason,
            threadId: payload.threadId,
            messageId: payload.messageId,
            kind: payload.interactionKind,
            toolCallId: payload.toolCallId ?? undefined,
            optionId: payload.reason === 'selected'
              ? payload.selectedOptionId ?? undefined
              : undefined,
            optionKind: payload.selectedOptionKind ?? undefined,
            optionLabel: payload.selectedOptionName ?? undefined,
          };
          const next = resolvedInteractionState(
            state.pendingPermissions,
            state.toolCalls,
            resolution,
          );
          const existing = state.pendingPermissions[payload.requestId];
          const kind = payload.interactionKind ?? existing?.kind;
          if (kind !== 'plan_review') return next;
          return {
            ...next,
            planDocumentsByThread: resolvePlanDocument(
              state.planDocumentsByThread,
              payload.requestId,
              {
                status: planDocumentStatusFromResolution(
                  resolution.optionId,
                  payload.reason,
                ),
                messageId: payload.messageId ?? existing?.messageId,
              },
            ),
          };
        });
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
        markTurnActivity(p.threadId);
        const statusRaw = (p.status ?? 'pending').toLowerCase();
        const status: AcpToolCallState['status'] =
          statusRaw === 'completed' || statusRaw === 'success'
            ? 'success'
            : statusRaw === 'failed' || statusRaw === 'error'
              ? 'error'
              : statusRaw === 'in_progress' || statusRaw === 'running'
                ? 'running'
                : 'queued';
        set((s) => {
          const toolKey = acpToolStateKey(p.threadId, p.toolCallId, p.messageId);
          const existing = s.toolCalls[toolKey];
          return {
            toolCalls: {
              ...s.toolCalls,
              [toolKey]: {
                ...existing,
                threadId: p.threadId,
                messageId: p.messageId,
                toolCallId: p.toolCallId,
                toolName: extractToolName(p.raw ?? {}, p.title),
                status,
                input: extractToolInput(p.raw ?? {}),
                output: extractToolOutput(p.raw ?? {}) ?? existing?.output,
              },
            },
          };
        });
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
        markTurnActivity(p.threadId);
        set((s) => {
          const toolKey = acpToolStateKey(p.threadId, p.toolCallId, p.messageId);
          const existing = s.toolCalls[toolKey];
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
          const output = extractToolOutput(p.raw ?? {});
          return {
            toolCalls: {
              ...s.toolCalls,
              [toolKey]: {
                ...existing,
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
          markTurnActivity(threadId);
          streamBatch.delete(messageId);
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
                      attachments: [],
                      meta_json: metaJson ?? null,
                      created_at: new Date().toISOString(),
                    },
                  ]
                : s.messages;
            return {
              streamingText: nextStreaming,
              statusByThread: { ...s.statusByThread, [threadId]: '' },
              runningByThread: { ...s.runningByThread, [threadId]: false },
              cancellingByThread: { ...s.cancellingByThread, [threadId]: false },
              planByThread: { ...s.planByThread, [threadId]: { entries: [], completed: 0, total: 0 } },
              // Keep plan documents for re-reading; only expire still-pending reviews.
              planDocumentsByThread: finalizePendingPlanDocuments(
                s.planDocumentsByThread,
                threadId,
              ),
              pendingPermissions: removeThreadEntries(s.pendingPermissions, threadId),
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
          markTurnActivity(threadId);
          if (messageId) streamBatch.delete(messageId);
          set((s) => {
            const nextStreaming = { ...s.streamingText };
            if (messageId) delete nextStreaming[messageId];
            const existing = messageId
              ? s.messages.find((item) => item.id === messageId)
              : undefined;
            const errorContent = text || existing?.content || `Error: ${message}`;
            const messages = messageId
              ? existing
                ? s.messages.map((item) => (
                    item.id === messageId
                      ? { ...item, content: errorContent, status: 'error' }
                      : item
                  ))
                : s.activeThreadId === threadId
                  ? [
                      ...s.messages,
                      {
                        id: messageId,
                        thread_id: threadId,
                        role: 'assistant',
                        content: errorContent,
                        status: 'error',
                        attachments: [],
                        created_at: new Date().toISOString(),
                      },
                    ]
                  : s.messages
              : s.messages;
            return {
              streamingText: nextStreaming,
              statusByThread: { ...s.statusByThread, [threadId]: message },
              runningByThread: { ...s.runningByThread, [threadId]: false },
              cancellingByThread: { ...s.cancellingByThread, [threadId]: false },
              planByThread: { ...s.planByThread, [threadId]: { entries: [], completed: 0, total: 0 } },
              planDocumentsByThread: finalizePendingPlanDocuments(
                s.planDocumentsByThread,
                threadId,
              ),
              pendingPermissions: removeThreadEntries(s.pendingPermissions, threadId),
              error: message,
              messages,
            };
          });
        },
      ),
    );

    // If a newer bindEvents started while we were awaiting listen(), drop these.
    if (!isLive()) {
      unlisteners.forEach((u) => u());
      return () => {};
    }

    const cleanup = () => {
      flushStreams();
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
        spawnModelByThread: s.spawnModelByThread,
        spawnReasoningByThread: s.spawnReasoningByThread,
      }),
    },
  ),
);
