import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import {
  App,
  Avatar,
  Button,
  Dropdown,
  Popover,
  Progress,
  Tooltip,
  Typography,
  theme,
  type MenuProps,
} from 'antd';
import Bubble from '@ant-design/x/es/bubble';
import type { BubbleItemType, BubbleListRef } from '@ant-design/x/es/bubble/interface';
import Actions from '@ant-design/x/es/actions';
import Prompts from '@ant-design/x/es/prompts';
import type { PromptsItemType } from '@ant-design/x/es/prompts';
import { setCustomComponents } from 'markstream-react';
import {
  ArrowUp,
  Bot,
  BrainCircuit,
  Bug,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy,
  GitBranch,
  GripHorizontal,
  Hammer,
  ListTodo,
  Paperclip,
  RefreshCw,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Square,
  Telescope,
  Timer,
  Upload,
  X,
  Zap,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@/lib/invoke';
import { useAcpStore } from '@/stores/acpStore';
import { useSettingsStore } from '@/stores';
import { useUserProfileStore } from '@/stores/userProfileStore';
import { useResolvedDarkMode } from '@/hooks/useResolvedDarkMode';
import { useResolvedAvatarSrc } from '@/hooks/useResolvedAvatarSrc';
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard';
import {
  ChatMarkdownRenderer,
  getChatCodeThemes,
  ThinkNode,
} from '@/components/chat/chatMarkdownShared';
import { ChatImageNode } from '@/components/chat/ChatImageNode';
import { ChatMessageRenderBoundary } from '@/components/chat/ChatMessageRenderBoundary';
import {
  AttachmentChips,
  createComposerAttachment,
  isImageFile,
  revokeComposerAttachments,
} from '@/components/chat/AttachmentChips';
import { MessageAttachmentPreview } from '@/components/chat/MessageAttachmentPreview';
import {
  fileToAttachmentInput,
  isAllowedAcpAttachmentFile,
  useComposerAttachments,
} from '@/components/chat/composerAttachments';
import { closeStreamingThinkBlock } from '@/components/chat/chatStreaming';
import {
  CHAT_AUTO_SCROLL_BOTTOM_THRESHOLD,
  CHAT_SCROLL_IS_REVERSED,
  shouldKeepAutoScroll,
  shouldShowScrollToBottom,
} from '@/components/chat/chatScroll';
import { AcpAgentIcon } from '@/lib/acpAgentIcon';
import type {
  AcpProject,
  AcpSessionConfigOption,
  AcpSessionConfigSelectOption,
} from '@/types/acp';
import { formatDurationI18n, parseAcpDurationMs } from '@/lib/formatDurationI18n';
import { normalizeThinkTagsForMarkdown } from '@/lib/thinkTags';
import {
  createPastedSnippet,
  insertPasteTokenAtSelection,
  isLongPastedText,
  mergePastedSnippetsIntoContent,
  removePasteTokens,
  type PastedSnippet,
} from '@/lib/pastedText';
import { AcpInteractionComposer } from './AcpInteractionComposer';
import { AcpPlanDocumentCard, setAcpPlanContextHandler } from './AcpPlanDocumentCard';
import { AcpPlanNode } from './AcpPlanNode';
import { AcpToolCallNode } from './AcpToolCallNode';
import { localizeAcpStatus } from './acpStatus';
import {
  AcpModelChoiceIcon,
  configChoicePayload,
  configChoices,
  formatAcpTime,
  isBooleanConfigOption,
  isDefaultAgentModeValue,
  isFullAccessPermissionChoice,
  isMaxThoughtLevel,
  isModelConfigExtra,
  isModelOption,
  isPermissionModeChoice,
  isPermissionOption,
  isPlanModeValue,
  isRestrictivePermissionChoice,
  isSpeedEnabled,
  isThoughtOption,
  modelIconKey,
  nextSpeedValue,
  optionContainsPlan,
  selectedConfigLabel,
} from './acpSessionConfig';

export { localizeAcpStatus } from './acpStatus';

const { Text, Title } = Typography;

/** Composer drag-resize (parity with chat InputArea). */
const COMPOSER_INITIAL_MIN_HEIGHT = 44;
const COMPOSER_ABSOLUTE_MAX_HEIGHT = 600;

function composerScopeKey(
  project: Pick<AcpProject, 'id' | 'kind'> | null,
  threadId: string | null,
): string {
  const projectScope = !threadId && (!project || project.kind !== 'project')
    ? 'recent'
    : (project?.id ?? '');
  return `${projectScope}:${threadId ?? 'draft'}`;
}

function mergeComposerRecoveryText(current: string, recovered: string): string {
  if (!recovered || current.includes(recovered)) return current;
  if (!current) return recovered;
  return `${current}${current.endsWith('\n') ? '\n' : '\n\n'}${recovered}`;
}

// Same markstream custom tags as chat (code/links use shared CSS via aqbot-chat-markdown)
setCustomComponents('acp', {
  think: ThinkNode,
  'tool-call': AcpToolCallNode,
  'acp-plan': AcpPlanNode,
  image: ChatImageNode,
  img: ChatImageNode,
});

function messageHasAcpPlanMarker(content: string | null | undefined, planId: string): boolean {
  if (!content || !planId) return false;
  const escaped = planId.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`<acp-plan\\b[^>]*\\bid="${escaped}"`, 'i').test(content);
}

/** Three-dot streaming indicator (matches ant Bubble loading dots style). */
function StreamingDots({ color }: { color?: string }) {
  return (
    <span
      aria-hidden
      className="aqbot-acp-streaming-dots"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        marginTop: 8,
        height: 14,
        color: color ?? 'currentColor',
      }}
    >
      {[0, 1, 2].map((i) => (
        <i
          key={i}
          style={{
            display: 'inline-block',
            width: 6,
            height: 6,
            borderRadius: '50%',
            background: 'currentColor',
            opacity: 0.35,
            animation: `aqbot-acp-dot-bounce 1.2s ease-in-out ${i * 0.16}s infinite`,
          }}
        />
      ))}
      <style>{`
        @keyframes aqbot-acp-dot-bounce {
          0%, 80%, 100% { transform: translateY(0); opacity: 0.3; }
          40% { transform: translateY(-3px); opacity: 0.85; }
        }
        @media (prefers-reduced-motion: reduce) {
          .aqbot-acp-streaming-dots i { animation: none !important; }
        }
      `}</style>
    </span>
  );
}

interface AcpGitInfo {
  branch: string | null;
  branches: string[];
  isRepo: boolean;
}

export function AcpConversationPane() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { modal, message: messageApi } = App.useApp();
  const themeMode = useSettingsStore((s) => s.settings.theme_mode);
  const isDarkMode = useResolvedDarkMode(themeMode ?? 'system');

  const projects = useAcpStore((s) => s.projects);
  const threads = useAcpStore((s) => s.threads);
  const messages = useAcpStore((s) => s.messages);
  const messagesLoadingByThread = useAcpStore((s) => s.messagesLoadingByThread);
  const messagesErrorByThread = useAcpStore((s) => s.messagesErrorByThread);
  const activeProjectId = useAcpStore((s) => s.activeProjectId);
  const activeThreadId = useAcpStore((s) => s.activeThreadId);
  const projectsReady = useAcpStore((s) => s.projectsReady);
  const threadsReady = useAcpStore((s) => s.threadsReady);
  const statusByThread = useAcpStore((s) => s.statusByThread);
  const runningByThread = useAcpStore((s) => s.runningByThread);
  const agentReadinessById = useAcpStore((s) => s.agentReadinessById);
  const sessionByThread = useAcpStore((s) => s.sessionByThread);
  const preparingByThread = useAcpStore((s) => s.preparingByThread);
  const cancellingByThread = useAcpStore((s) => s.cancellingByThread);
  const planByThread = useAcpStore((s) => s.planByThread);
  const planDocumentsByThread = useAcpStore((s) => s.planDocumentsByThread);
  const pendingPermissions = useAcpStore((s) => s.pendingPermissions);
  const loadMessages = useAcpStore((s) => s.loadMessages);
  const sendPrompt = useAcpStore((s) => s.sendPrompt);
  const createThread = useAcpStore((s) => s.createThread);
  const ensureRecentDraft = useAcpStore((s) => s.ensureRecentDraft);
  const selectProject = useAcpStore((s) => s.selectProject);
  const prepareDraft = useAcpStore((s) => s.prepareDraft);
  const prepareSession = useAcpStore((s) => s.prepareSession);
  const setConfigOption = useAcpStore((s) => s.setConfigOption);
  const setSessionMode = useAcpStore((s) => s.setSessionMode);
  const cancelPrompt = useAcpStore((s) => s.cancelPrompt);
  const respondPermission = useAcpStore((s) => s.respondPermission);
  const cancelInteraction = useAcpStore((s) => s.cancelInteraction);
  const respondQuestionnaire = useAcpStore((s) => s.respondQuestionnaire);
  const enabledAgents = useAcpStore((s) => s.enabledAgents);
  const saveComposerDraft = useAcpStore((s) => s.saveComposerDraft);
  const takeComposerDraft = useAcpStore((s) => s.takeComposerDraft);
  const takeComposerRecovery = useAcpStore((s) => s.takeComposerRecovery);
  const clearComposerDraft = useAcpStore((s) => s.clearComposerDraft);

  const settings = useSettingsStore((s) => s.settings);
  /** Follow conversation settings (modern / compact / minimal), same as ChatView. */
  const bubbleStyle = settings.bubble_style || 'modern';
  const profile = useUserProfileStore((s) => s.profile);
  const resolvedAvatarSrc = useResolvedAvatarSrc(profile.avatarType, profile.avatarValue);
  const { copy: copyText, isCopiedFor } = useCopyToClipboard();
  const localizedStatus = useCallback(
    (status: string | undefined) => localizeAcpStatus(
      status,
      (key, values) => t(key, values),
    ),
    [t],
  );

  const getBubbleVariant = useCallback(
    (isUser: boolean): {
      variant: 'filled' | 'outlined' | 'shadow' | 'borderless';
      style?: CSSProperties;
    } => {
      switch (bubbleStyle) {
        case 'compact':
          return { variant: 'borderless' };
        case 'minimal':
          return { variant: 'borderless', style: { padding: '4px 8px' } };
        case 'modern':
        default:
          return { variant: isUser ? 'shadow' : 'outlined' };
      }
    },
    [bubbleStyle],
  );

  const [value, setValue] = useState('');
  const valueRef = useRef(value);
  valueRef.current = value;
  const [pastedSnippets, setPastedSnippets] = useState<PastedSnippet[]>([]);
  const pastedSnippetsRef = useRef(pastedSnippets);
  pastedSnippetsRef.current = pastedSnippets;
  const pastedSnippetSeqRef = useRef(0);
  const [sending, setSending] = useState(false);
  const [composerAgentId, setComposerAgentId] = useState<string | null>(null);
  const [gitInfo, setGitInfo] = useState<AcpGitInfo | null>(null);
  const [gitLoading, setGitLoading] = useState(false);
  const [checkoutLoading, setCheckoutLoading] = useState(false);
  const [configUpdatingBySession, setConfigUpdatingBySession] = useState<Record<string, string>>({});
  const [recentDraftPreparing, setRecentDraftPreparing] = useState(false);
  const [recentDraftError, setRecentDraftError] = useState<string | null>(null);
  const activeProjectIdRef = useRef(activeProjectId);
  activeProjectIdRef.current = activeProjectId;
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const lastNonPlanModeBySessionRef = useRef<Record<string, string>>({});
  const bubbleListRef = useRef<BubbleListRef | null>(null);
  const stickToBottomRef = useRef(true);
  const [showScrollToBottom, setShowScrollToBottom] = useState(false);

  // Drag-to-resize composer (parity with chat InputArea)
  const [userMinHeight, setUserMinHeight] = useState(COMPOSER_INITIAL_MIN_HEIGHT);
  const userMinHeightRef = useRef(userMinHeight);
  userMinHeightRef.current = userMinHeight;
  const dragStateRef = useRef<{ startY: number; startH: number } | null>(null);
  const resizeCleanupRef = useRef<() => void>(() => {});
  const hasUserResizedRef = useRef(false);

  const agents = enabledAgents();
  const activeProject = projects.find((p) => p.id === activeProjectId) ?? null;
  const activeThread = threads.find((th) => th.id === activeThreadId) ?? null;
  const messagesLoading = !!(
    activeThreadId && messagesLoadingByThread[activeThreadId]
  );
  const messagesError = activeThreadId
    ? messagesErrorByThread[activeThreadId]
    : undefined;
  const streaming = !!(
    activeThreadId
    && (
      runningByThread[activeThreadId]
      || messages.some(
        (message) => message.thread_id === activeThreadId
          && message.role === 'assistant'
          && message.status === 'streaming',
      )
    )
  );
  const pendingInteractions = useMemo(() => (
    Object.values(pendingPermissions)
      .filter((request) => (
        request.threadId === activeThreadId && request.status === 'pending'
      ))
      .sort((left, right) => (
        (left.sequence ?? Number.MAX_SAFE_INTEGER)
        - (right.sequence ?? Number.MAX_SAFE_INTEGER)
      ))
  ), [activeThreadId, pendingPermissions]);
  const [activeInteractionId, setActiveInteractionId] = useState<string | null>(null);
  const previousInteractionIndexRef = useRef(0);
  const selectedInteractionIndex = activeInteractionId
    ? pendingInteractions.findIndex((request) => request.requestId === activeInteractionId)
    : -1;
  const clampedInteractionIndex = selectedInteractionIndex >= 0
    ? selectedInteractionIndex
    : 0;
  const activeInteraction = pendingInteractions[clampedInteractionIndex] ?? null;
  if (selectedInteractionIndex >= 0) {
    previousInteractionIndexRef.current = selectedInteractionIndex;
  }
  const previousInteractionIdRef = useRef<string | null>(null);

  useEffect(() => {
    previousInteractionIndexRef.current = 0;
    setActiveInteractionId(null);
  }, [activeThreadId]);

  useEffect(() => {
    setActiveInteractionId((current) => {
      if (pendingInteractions.length === 0) return null;
      if (current && pendingInteractions.some((request) => request.requestId === current)) {
        return current;
      }
      const adjacentIndex = Math.min(
        previousInteractionIndexRef.current,
        pendingInteractions.length - 1,
      );
      return pendingInteractions[adjacentIndex].requestId;
    });
  }, [pendingInteractions]);

  useEffect(() => {
    const previousId = previousInteractionIdRef.current;
    const currentId = activeInteraction?.requestId ?? null;
    previousInteractionIdRef.current = currentId;
    if (!previousId || currentId) return undefined;
    const frame = window.requestAnimationFrame(() => textareaRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [activeInteraction?.requestId]);

  // Prefer thread agent; otherwise composer selection / first enabled agent
  const selectedComposerAgentId = agents.some((agent) => agent.id === composerAgentId)
    ? composerAgentId
    : null;
  const effectiveAgentId =
    activeThread?.agent_id
    ?? selectedComposerAgentId
    ?? agents[0]?.id
    ?? null;
  const agentMeta = agents.find((a) => a.id === effectiveAgentId);
  const draftKey = activeProjectId && effectiveAgentId
    ? `draft:${activeProjectId}:${effectiveAgentId}`
    : null;
  const sessionKey = activeThreadId ?? draftKey;
  const sessionKeyRef = useRef(sessionKey);
  sessionKeyRef.current = sessionKey;
  const configUpdatingId = sessionKey
    ? configUpdatingBySession[sessionKey] ?? null
    : null;
  const sessionSnapshot = sessionKey ? sessionByThread[sessionKey] : undefined;
  const agentProcessReady = !!(
    effectiveAgentId && agentReadinessById[effectiveAgentId]?.status === 'ready'
  );
  const preparing = !!(sessionKey && preparingByThread[sessionKey]);
  const cancelling = !!(activeThreadId && cancellingByThread[activeThreadId]);
  const activePlan = activeThreadId ? planByThread[activeThreadId] : undefined;
  const supportsImageAttachments =
    sessionSnapshot?.agentCapabilities.promptCapabilities?.image === true;
  const acceptAcpAttachment = useCallback(
    (file: File) => isAllowedAcpAttachmentFile(file, supportsImageAttachments),
    [supportsImageAttachments],
  );
  const handleRejectedAttachments = useCallback(() => {
    messageApi.warning(t('agentPage.imageAttachmentUnsupported'));
  }, [messageApi, t]);
  const handleAttachmentReadError = useCallback((filePath: string, error: unknown) => {
    console.error('[acp attachment] Failed to read file:', filePath, error);
    const name = filePath.split(/[\\/]/).pop() || filePath || t('common.unknown');
    messageApi.error(t('agentPage.attachmentReadFailed', { name }));
  }, [messageApi, t]);
  const {
    attachments: attachedFiles,
    attachmentsRef,
    fileInputRef,
    isDragging,
    removeAttachment,
    resetAttachments,
    detachAttachments,
    restoreAttachments,
    openFilePicker,
    handleFileChange,
    handleClipboardFiles,
    dragHandlers,
  } = useComposerAttachments({
    enabled: !!effectiveAgentId,
    acceptFile: acceptAcpAttachment,
    onRejected: handleRejectedAttachments,
    onReadError: handleAttachmentReadError,
  });

  const currentComposerScopeKey = composerScopeKey(activeProject, activeThreadId);
  const composerRecoveryId = useAcpStore(
    (s) => s.composerDraftsByScope[currentComposerScopeKey]?.recovery?.id,
  );
  const composerScopeRef = useRef(currentComposerScopeKey);
  composerScopeRef.current = currentComposerScopeKey;
  const previousComposerScopeRef = useRef<string | null>(null);

  useEffect(() => {
    const resetDraft = () => {
      clearComposerDraft(composerScopeRef.current);
      valueRef.current = '';
      pastedSnippetsRef.current = [];
      setValue('');
      setPastedSnippets([]);
      pastedSnippetSeqRef.current = 0;
      resetAttachments();
      requestAnimationFrame(() => textareaRef.current?.focus());
    };
    window.addEventListener('aqbot:reset-agent-draft', resetDraft);
    return () => window.removeEventListener('aqbot:reset-agent-draft', resetDraft);
  }, [clearComposerDraft, resetAttachments]);

  useLayoutEffect(() => () => {
    saveComposerDraft(composerScopeRef.current, {
      value: valueRef.current,
      snippets: pastedSnippetsRef.current,
      files: attachmentsRef.current.map(({ file }) => file),
    });
  }, [attachmentsRef, saveComposerDraft]);

  useEffect(() => {
    if (!composerAgentId && agents[0]?.id) {
      setComposerAgentId(agents[0].id);
    }
  }, [agents, composerAgentId]);

  // When opening a thread, sync composer agent to that thread's agent
  useEffect(() => {
    if (activeThread?.agent_id) {
      setComposerAgentId(activeThread.agent_id);
    }
  }, [activeThread?.agent_id]);

  useEffect(() => {
    const previousScope = previousComposerScopeRef.current;
    if (previousScope === currentComposerScopeKey) return;
    if (previousScope) {
      const previousAttachments = detachAttachments();
      saveComposerDraft(previousScope, {
        value: valueRef.current,
        snippets: pastedSnippetsRef.current,
        files: previousAttachments.map(({ file }) => file),
      });
      revokeComposerAttachments(previousAttachments);
    }

    const nextDraft = takeComposerDraft(currentComposerScopeKey);
    const nextValue = mergeComposerRecoveryText(
      nextDraft?.value ?? '',
      nextDraft?.recovery?.text ?? '',
    );
    const nextSnippets = nextDraft?.snippets ?? [];
    valueRef.current = nextValue;
    pastedSnippetsRef.current = nextSnippets;
    setValue(nextValue);
    setPastedSnippets(nextSnippets);
    pastedSnippetSeqRef.current = nextSnippets.reduce(
      (maximum, snippet) => Math.max(maximum, snippet.index),
      0,
    );
    if (nextDraft?.files.length) {
      restoreAttachments(nextDraft.files.map((file) => createComposerAttachment(file)));
    }
    if (nextDraft?.recovery) messageApi.error(nextDraft.recovery.error);
    previousComposerScopeRef.current = currentComposerScopeKey;
  }, [
    currentComposerScopeKey,
    detachAttachments,
    messageApi,
    restoreAttachments,
    saveComposerDraft,
    takeComposerDraft,
  ]);

  const prepareRecentWorkspace = useCallback(async () => {
    setRecentDraftPreparing(true);
    setRecentDraftError(null);
    try {
      await ensureRecentDraft();
    } catch (error) {
      setRecentDraftError(String(error));
    } finally {
      setRecentDraftPreparing(false);
    }
  }, [ensureRecentDraft]);

  // Warm initialize + session setup while the user is reading/typing. The
  // store deduplicates StrictMode and rapid-selection calls.
  useEffect(() => {
    if (activeThreadId) {
      if (!sessionByThread[activeThreadId]) {
        void prepareSession(activeThreadId).catch(() => undefined);
      }
      return;
    }
    if (!activeProjectId && effectiveAgentId && projectsReady && threadsReady) {
      void prepareRecentWorkspace();
      return;
    }
    if (activeProjectId && effectiveAgentId) {
      const key = `draft:${activeProjectId}:${effectiveAgentId}`;
      if (!sessionByThread[key]) {
        void prepareDraft(activeProjectId, effectiveAgentId).catch(() => undefined);
      }
    }
  }, [
    activeThreadId,
    activeProjectId,
    effectiveAgentId,
    prepareRecentWorkspace,
    prepareDraft,
    prepareSession,
    projectsReady,
    sessionByThread,
    threadsReady,
  ]);

  // Load git branch info for active project
  useEffect(() => {
    setCheckoutLoading(false);
    setGitInfo(null);
    if (!activeProjectId) {
      return;
    }
    let cancelled = false;
    setGitLoading(true);
    void invoke<AcpGitInfo>('acp_git_info', { projectId: activeProjectId })
      .then((info) => {
        if (!cancelled) setGitInfo(info);
      })
      .catch(() => {
        if (!cancelled) setGitInfo({ branch: null, branches: [], isRepo: false });
      })
      .finally(() => {
        if (!cancelled) setGitLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeProjectId]);

  const { darkTheme, lightTheme, themes } = useMemo(
    () => getChatCodeThemes(settings.code_theme, settings.code_theme_light),
    [settings.code_theme, settings.code_theme_light],
  );

  const userAvatar = useMemo(() => {
    if (profile.avatarType === 'emoji' && profile.avatarValue) {
      return (
        <Avatar size={32} style={{ backgroundColor: token.colorPrimaryBg, fontSize: 16 }}>
          {profile.avatarValue}
        </Avatar>
      );
    }
    if ((profile.avatarType === 'url' || profile.avatarType === 'file') && profile.avatarValue) {
      const src =
        profile.avatarType === 'file'
          ? (resolvedAvatarSrc ?? (profile.avatarValue.startsWith('data:') ? profile.avatarValue : undefined))
          : profile.avatarValue;
      return <Avatar size={32} src={src} />;
    }
    return (
      <Avatar size={32} style={{ backgroundColor: token.colorPrimary }}>
        {(profile.name || 'U')[0]}
      </Avatar>
    );
  }, [profile, resolvedAvatarSrc, token.colorPrimary, token.colorPrimaryBg]);

  const agentAvatar = useMemo(() => {
    if (!effectiveAgentId) return <Avatar size={32} icon={<Bot size={16} />} />;
    return (
      <AcpAgentIcon
        agentId={effectiveAgentId}
        agentName={agentMeta?.name}
        icon={agentMeta?.icon}
        size={32}
      />
    );
  }, [effectiveAgentId, agentMeta?.name, agentMeta?.icon]);

  const planDocuments = useMemo(() => {
    if (!activeThreadId) return [];
    return [...(planDocumentsByThread[activeThreadId] ?? [])]
      .sort((left, right) => left.sequence - right.sequence);
  }, [activeThreadId, planDocumentsByThread]);

  /** Attach a resolved plan body to the composer as a paste snippet (context). */
  const addPlanToContext = useCallback((content: string) => {
    const text = content.trim();
    if (!text) return;
    pastedSnippetSeqRef.current += 1;
    const index = pastedSnippetSeqRef.current;
    setPastedSnippets((previous) => [...previous, createPastedSnippet(text, index)]);
    setValue((current) => {
      const start = current.length;
      const inserted = insertPasteTokenAtSelection(current, start, start, index);
      return inserted.value;
    });
    messageApi.success(t('agentPage.interactionPlanAddedToContext'));
    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      if (!textarea) return;
      textarea.focus();
      textarea.style.height = 'auto';
      const desired = hasUserResizedRef.current
        ? userMinHeightRef.current
        : Math.max(textarea.scrollHeight, userMinHeightRef.current);
      textarea.style.height = `${Math.min(desired, COMPOSER_ABSOLUTE_MAX_HEIGHT)}px`;
    });
  }, [messageApi, t]);

  // Inline <acp-plan> nodes call this via the shared handler registry.
  useEffect(() => {
    setAcpPlanContextHandler(addPlanToContext);
    return () => setAcpPlanContextHandler(null);
  }, [addPlanToContext]);

  const bubbleItems: BubbleItemType[] = useMemo(() => {
    // Plans with inline <acp-plan> markers render inside the message body
    // (chronological). Legacy plans without markers still get a fallback bubble
    // after their host message.
    const items: BubbleItemType[] = [];
    const attachedPlanIds = new Set<string>();

    for (const message of messages) {
      items.push({
        key: message.id,
        role: message.role === 'user' ? 'user' : 'ai',
        content: message.content ?? '',
        // ACP owns its empty-stream renderer below. Ant Bubble's built-in
        // loading branch bypasses contentRender and hides status/permissions.
        loading: false,
      });

      for (const plan of planDocuments) {
        if (plan.messageId !== message.id) continue;
        // Pending reviews already occupy the composer — avoid duplicate body.
        if (plan.status === 'pending') continue;
        // Already placed chronologically inside the assistant message.
        if (messageHasAcpPlanMarker(message.content, plan.id)) {
          attachedPlanIds.add(plan.id);
          continue;
        }
        attachedPlanIds.add(plan.id);
        items.push({
          key: `plan:${plan.id}`,
          role: 'plan',
          content: plan.content,
          loading: false,
        });
      }
    }

    // Plans without a message id (or whose message is not loaded yet) still
    // appear at the end so the body remains readable after leaving plan mode.
    for (const plan of planDocuments) {
      if (attachedPlanIds.has(plan.id) || plan.status === 'pending') continue;
      items.push({
        key: `plan:${plan.id}`,
        role: 'plan',
        content: plan.content,
        loading: false,
      });
    }

    return items;
  }, [messages, planDocuments]);

  const renderMessageHeader = useCallback(
    (msgId: string, role: 'user' | 'assistant') => {
      const msg = messages.find((m) => m.id === msgId);
      const name = role === 'user'
        ? (profile.name || t('chat.you'))
        : (agentMeta?.name || activeThread?.agent_id || 'Agent');
      return (
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <Text style={{ fontSize: 13 }}>{name}</Text>
          {/* Match ChatView: timestamp sits next to the name, not under content */}
          {msg ? (
            <Text type="secondary" style={{ fontSize: 11 }}>
              {formatAcpTime(msg.created_at)}
            </Text>
          ) : null}
        </div>
      );
    },
    [messages, profile.name, t, agentMeta?.name, activeThread?.agent_id],
  );

  const renderMessageFooter = useCallback(
    (msgId: string, content: string, role: 'user' | 'assistant') => {
      const msg = messages.find((m) => m.id === msgId);
      if (!msg) return null;
      const isStreamingMsg =
        msg.status === 'streaming'
        || (streaming && msg.id === messages[messages.length - 1]?.id && msg.role === 'assistant');
      if (isStreamingMsg) return null;

      const plainCopy = content
        .replace(/<tool-call\b[^>]*>[\s\S]*?<\/tool-call>/gi, '')
        .replace(/<acp-plan\b[^>]*>[\s\S]*?<\/acp-plan>/gi, '')
        .replace(/<tool-call\b[^>]*\/?>/gi, '')
        .replace(/<acp-plan\b[^>]*\/?>/gi, '')
        .trim() || content;
      const copied = isCopiedFor(plainCopy);
      const durationMs = role === 'assistant' ? parseAcpDurationMs(msg.meta_json) : null;
      const durationLabel =
        durationMs != null && durationMs > 0 ? formatDurationI18n(durationMs, t) : null;

      return (
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            flexWrap: 'wrap',
            marginTop: 2,
          }}
        >
          {durationLabel ? (
            <Text
              type="secondary"
              style={{ fontSize: 11, display: 'inline-flex', alignItems: 'center', gap: 3 }}
            >
              <Timer size={11} />
              {durationLabel}
            </Text>
          ) : null}
          <Actions
            items={[
              {
                key: 'copy',
                icon: copied
                  ? <Check size={14} style={{ color: token.colorSuccess }} />
                  : <Copy size={14} />,
                label: t('chat.copy'),
                onItemClick: () => {
                  void copyText(plainCopy).then((ok) => {
                    if (ok) messageApi.success(t('chat.copied'));
                  });
                },
              },
            ]}
          />
        </div>
      );
    },
    [messages, streaming, isCopiedFor, t, token.colorSuccess, copyText, messageApi],
  );

  const roles = useMemo(() => ({
    user: {
      placement: 'end' as const,
      shape: 'corner' as const,
      ...getBubbleVariant(true),
      avatar: userAvatar,
      header: (_content: string, info: { key?: string | number }) =>
        renderMessageHeader(String(info.key ?? ''), 'user'),
      contentRender: (content: string, info: { key?: string | number }) => {
        const message = messages.find((item) => item.id === String(info.key));
        const attachments = message?.attachments ?? [];
        return (
          <div style={{ textAlign: 'right' }}>
            {content ? (
              <div
                className="aqbot-chat-text"
                style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}
              >
                {content}
              </div>
            ) : null}
            {attachments.length > 0 ? (
              <div
                style={{
                  display: 'flex',
                  flexWrap: 'wrap',
                  justifyContent: 'flex-end',
                  gap: 8,
                  marginTop: content ? 8 : 0,
                }}
              >
                {attachments.map((attachment) => (
                  <MessageAttachmentPreview
                    key={attachment.id}
                    attachment={attachment}
                    themeColor={token.colorPrimary}
                  />
                ))}
              </div>
            ) : null}
          </div>
        );
      },
      footer: (content: string, info: { key?: string | number }) =>
        renderMessageFooter(String(info.key ?? ''), String(content ?? ''), 'user'),
    },
    ai: {
      placement: 'start' as const,
      shape: 'corner' as const,
      ...getBubbleVariant(false),
      avatar: agentAvatar,
      header: (_content: string, info: { key?: string | number }) =>
        renderMessageHeader(String(info.key ?? ''), 'assistant'),
      contentRender: (content: string, { key }: { key?: string | number }) => {
        const msg = messages.find((m) => m.id === String(key));
        const isStreamingMsg =
          msg?.status === 'streaming'
          || (streaming && msg?.id === messages[messages.length - 1]?.id && msg?.role === 'assistant');
        const body = normalizeThinkTagsForMarkdown(
          closeStreamingThinkBlock(content || '', isStreamingMsg),
        );

        // Empty bubble still streaming → status + dots. Pending interactions
        // take over the composer instead of being appended to this message.
        if (!body && isStreamingMsg) {
          return (
            <div>
              <Text type="secondary" style={{ fontSize: 13 }}>
                {localizedStatus(statusByThread[activeThreadId ?? ''])
                  || t('agentPage.streaming')}
              </Text>
              <StreamingDots color={token.colorTextSecondary} />
            </div>
          );
        }

        return (
          <div>
            {body ? (
              <ChatMessageRenderBoundary
                fallback={
                  <div style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{content}</div>
                }
              >
                {/*
                  aqbot-chat-markdown applies the same typography / code / link styles
                  as the chat module; customId "acp" scopes tool-call / think nodes.
                */}
                <div className="aqbot-chat-markdown">
                  <ChatMarkdownRenderer
                    content={body}
                    isDark={isDarkMode}
                    final={!isStreamingMsg}
                    codeBlockDarkTheme={darkTheme}
                    codeBlockLightTheme={lightTheme}
                    codeBlockThemes={themes}
                    codeFontFamily={settings.code_font_family || undefined}
                    customId="acp"
                  />
                </div>
              </ChatMessageRenderBoundary>
            ) : null}
            {isStreamingMsg ? <StreamingDots color={token.colorTextSecondary} /> : null}
          </div>
        );
      },
      footer: (content: string, info: { key?: string | number }) =>
        renderMessageFooter(String(info.key ?? ''), String(content ?? ''), 'assistant'),
    },
    // Resolved plan-review cards sit in the message timeline (not the composer).
    plan: {
      placement: 'start' as const,
      shape: 'corner' as const,
      variant: 'borderless' as const,
      avatar: <span style={{ width: 32, display: 'inline-block' }} />,
      header: () => null,
      contentRender: (_content: string, info: { key?: string | number }) => {
        const planId = String(info.key ?? '').replace(/^plan:/, '');
        const document = planDocuments.find((item) => item.id === planId);
        if (!document) return null;
        return (
          <div style={{ width: '100%', minWidth: 0, maxWidth: '100%' }}>
            <AcpPlanDocumentCard
              document={document}
              onAddToContext={addPlanToContext}
            />
          </div>
        );
      },
      footer: () => null,
    },
  }), [
    userAvatar,
    agentAvatar,
    messages,
    planDocuments,
    addPlanToContext,
    t,
    isDarkMode,
    darkTheme,
    lightTheme,
    themes,
    settings.code_font_family,
    streaming,
    statusByThread,
    activeThreadId,
    token.colorTextSecondary,
    token.colorPrimary,
    renderMessageHeader,
    renderMessageFooter,
    getBubbleVariant,
    localizedStatus,
  ]);

  const configOptions = sessionSnapshot?.configOptions ?? [];
  const planOption = configOptions.find(
    (option) => optionContainsPlan(option),
  );
  const advertisedPermissionOption = configOptions.find(
    (option) => option !== planOption && isPermissionOption(option),
  ) ?? (planOption && isPermissionOption(planOption) ? planOption : undefined);
  const sessionModePermissionChoices = (sessionSnapshot?.modes?.availableModes ?? [])
    .filter((mode) => !isPlanModeValue(mode.id))
    .filter((mode) => (
      isDefaultAgentModeValue(mode.id)
      || isDefaultAgentModeValue(mode.name)
      || isPermissionModeChoice(mode.id, mode.name)
    ))
    .filter((mode) => mode.id && mode.name);
  const hasSessionModePermissions = sessionModePermissionChoices.length >= 2
    && sessionModePermissionChoices.some(
      (mode) => isPermissionModeChoice(mode.id, mode.name),
    );
  const sessionModePermissionOption: AcpSessionConfigOption | undefined =
    !advertisedPermissionOption && hasSessionModePermissions
      ? {
          id: '__session_permission_mode',
          name: 'Permission',
          category: 'mode',
          type: 'select',
          currentValue: sessionModePermissionChoices.some(
            (mode) => mode.id === sessionSnapshot?.modes?.currentModeId,
          )
            ? String(sessionSnapshot?.modes?.currentModeId)
            : sessionModePermissionChoices.find(
                (mode) => isDefaultAgentModeValue(mode.id),
              )?.id ?? sessionModePermissionChoices[0].id,
          options: sessionModePermissionChoices.map((mode) => ({
            value: mode.id,
            name: mode.name,
            description: mode.description,
          })),
        }
      : undefined;
  const permissionOption = advertisedPermissionOption ?? sessionModePermissionOption;
  const permissionUsesSessionMode = permissionOption === sessionModePermissionOption;
  const modelOption = configOptions.find((option) => isModelOption(option));
  const thoughtOption = configOptions.find((option) => isThoughtOption(option));
  const modelConfigExtras = configOptions.filter(
    (option) =>
      option !== planOption
      && option !== permissionOption
      && option !== modelOption
      && option !== thoughtOption
      && isModelConfigExtra(option),
  );
  const planMode = sessionSnapshot?.modes?.availableModes.find(
    (mode) => isPlanModeValue(mode.id) || isPlanModeValue(mode.name),
  );
  const planEnabled = planMode
    ? sessionSnapshot?.modes?.currentModeId === planMode.id
    : !!planOption && isPlanModeValue(planOption.currentValue);
  const planModeToggleDisabled = !sessionKey
    || sending
    || streaming
    || preparing
    || !!configUpdatingId;
  const permissionChoices = configChoices(permissionOption).filter(
    (choice) => permissionOption !== planOption || !isPlanModeValue(choice.value),
  );

  useEffect(() => {
    if (!sessionKey) return;
    const currentMode = sessionSnapshot?.modes?.currentModeId;
    if (currentMode && !isPlanModeValue(currentMode)) {
      lastNonPlanModeBySessionRef.current[sessionKey] = currentMode;
      return;
    }
    const currentConfig = permissionOption?.currentValue;
    if (typeof currentConfig === 'string' && !isPlanModeValue(currentConfig)) {
      lastNonPlanModeBySessionRef.current[sessionKey] = currentConfig;
    }
  }, [permissionOption?.currentValue, sessionKey, sessionSnapshot?.modes?.currentModeId]);

  const permissionChoiceName = useCallback((choice: AcpSessionConfigSelectOption) => {
    const token = String(choice.value).trim().toLowerCase().replace(/[\s_-]/g, '');
    if (token === 'dontask') {
      return t('agent.permissionDontAsk');
    }
    if (token === 'auto') {
      return t('agent.permissionAutoApprove');
    }
    if (token === 'acceptedits' || token === 'autoedit' || token === 'agent') {
      return t('agent.permissionAcceptEdits');
    }
    if (token === 'bypasspermissions' || token === 'dangerouslyskippermissions') {
      return t('agent.permissionFullAccess');
    }
    if (isRestrictivePermissionChoice(choice.value, choice.name)) {
      return t('agent.permissionAskEveryTime');
    }
    if (isFullAccessPermissionChoice(choice.value, choice.name)) {
      return t('agent.permissionFullAccess');
    }
    return choice.name;
  }, [t]);

  const configChoiceName = useCallback((
    option: AcpSessionConfigOption | undefined,
    choice: AcpSessionConfigSelectOption,
  ) => {
    if (option && isPermissionOption(option)) return permissionChoiceName(choice);
    if (option && isBooleanConfigOption(option)) {
      if (choice.value === 'true') return t('common.on');
      if (choice.value === 'false') return t('common.off');
    }
    if (choice.value === '__agent_default') {
      return t('agent.agentDefault');
    }
    return choice.name;
  }, [permissionChoiceName, t]);

  const choiceItems = useCallback(
    (
      option?: AcpSessionConfigOption,
      choices: AcpSessionConfigSelectOption[] = configChoices(option),
    ): MenuProps['items'] => choices.map((choice) => {
      const showModelIcon = !!option && isModelOption(option);
      return {
        key: String(choice.value),
        label: (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            {showModelIcon ? (
              <span style={{ flexShrink: 0, lineHeight: 0 }}>
                <AcpModelChoiceIcon
                  modelId={modelIconKey(choice)}
                  agentId={effectiveAgentId}
                  agentName={agentMeta?.name}
                  agentIcon={agentMeta?.icon}
                  size={16}
                />
              </span>
            ) : null}
            <span style={{ minWidth: 0 }}>{configChoiceName(option, choice)}</span>
          </div>
        ),
      };
    }),
    [agentMeta?.icon, agentMeta?.name, configChoiceName, effectiveAgentId],
  );

  const selectedOptionLabel = useCallback((option?: AcpSessionConfigOption) => {
    if (!option) return selectedConfigLabel(option);
    const current = configChoices(option).find(
      (choice) => String(choice.value) === String(option.currentValue),
    );
    return current ? configChoiceName(option, current) : selectedConfigLabel(option);
  }, [configChoiceName]);

  const rememberedPermissionValue = sessionKey
    ? lastNonPlanModeBySessionRef.current[sessionKey]
    : undefined;
  const selectedPermissionValue = planEnabled
    ? permissionUsesSessionMode || permissionOption === planOption
      ? rememberedPermissionValue
        ?? permissionChoices.find((choice) => isDefaultAgentModeValue(choice.value))?.value
        ?? permissionChoices[0]?.value
      : permissionOption?.currentValue
    : permissionOption?.currentValue;
  const selectedPermissionChoice = permissionChoices.find(
    (choice) => String(choice.value) === String(selectedPermissionValue),
  );
  const selectedPermissionLabel = selectedPermissionChoice
    ? configChoiceName(permissionOption, selectedPermissionChoice)
    : selectedOptionLabel(permissionOption);

  const applyConfigChoice = useCallback(async (configId: string, choice: string | boolean) => {
    if (!sessionKey || configUpdatingId) return;
    const targetSessionKey = sessionKey;
    setConfigUpdatingBySession((current) => ({
      ...current,
      [targetSessionKey]: configId,
    }));
    try {
      await setConfigOption(targetSessionKey, configId, choice);
    } catch (error) {
      if (sessionKeyRef.current === targetSessionKey) {
        messageApi.error(String(error));
      }
    } finally {
      setConfigUpdatingBySession((current) => {
        if (current[targetSessionKey] !== configId) return current;
        const { [targetSessionKey]: _completed, ...remaining } = current;
        return remaining;
      });
    }
  }, [configUpdatingId, messageApi, sessionKey, setConfigOption]);

  const applySessionModeChoice = useCallback(async (modeId: string, updateId: string) => {
    if (!sessionKey || configUpdatingId) return;
    const targetSessionKey = sessionKey;
    setConfigUpdatingBySession((current) => ({
      ...current,
      [targetSessionKey]: updateId,
    }));
    try {
      await setSessionMode(targetSessionKey, modeId);
    } catch (error) {
      if (sessionKeyRef.current === targetSessionKey) {
        messageApi.error(String(error));
      }
    } finally {
      setConfigUpdatingBySession((current) => {
        if (current[targetSessionKey] !== updateId) return current;
        const { [targetSessionKey]: _completed, ...remaining } = current;
        return remaining;
      });
    }
  }, [configUpdatingId, messageApi, sessionKey, setSessionMode]);

  const handlePermissionChange = useCallback((choiceId: string) => {
    if (!permissionOption) return;
    const choice = configChoices(permissionOption).find((item) => item.value === choiceId);
    const needsConfirmation = !isRestrictivePermissionChoice(choiceId, choice?.name);
    const apply = () => permissionUsesSessionMode
      ? applySessionModeChoice(choiceId, permissionOption.id)
      : applyConfigChoice(
          permissionOption.id,
          configChoicePayload(permissionOption, choiceId),
        );
    if (!needsConfirmation) {
      void apply();
      return;
    }
    const fullAccess = isFullAccessPermissionChoice(choiceId, choice?.name)
      || permissionOption.id.toLowerCase().includes('allow_all');
    modal.confirm({
      title: fullAccess
        ? t('agent.permissionFullAccessWarningTitle')
        : t('agent.permissionAcceptEditsWarningTitle'),
      content: choice?.description
        ?? t('agent.permissionAcceptEditsWarning'),
      okText: t('common.confirm'),
      cancelText: t('common.cancel'),
      okButtonProps: fullAccess ? { danger: true } : undefined,
      onOk: apply,
    });
  }, [
    applyConfigChoice,
    applySessionModeChoice,
    modal,
    permissionOption,
    permissionUsesSessionMode,
    t,
  ]);

  const setPlanModeEnabled = useCallback(async (enabled: boolean) => {
    if (planModeToggleDisabled) return;
    if (planMode) {
      const currentlyOn = sessionSnapshot?.modes?.currentModeId === planMode.id;
      if (enabled === currentlyOn) return;
      if (enabled && sessionKey) {
        const currentMode = sessionSnapshot?.modes?.currentModeId;
        if (currentMode && !isPlanModeValue(currentMode)) {
          lastNonPlanModeBySessionRef.current[sessionKey] = currentMode;
        }
      }
      const rememberedMode = sessionKey
        ? lastNonPlanModeBySessionRef.current[sessionKey]
        : undefined;
      const target = enabled
        ? planMode
        : sessionSnapshot?.modes?.availableModes.find((mode) => mode.id === rememberedMode)
          ?? sessionSnapshot?.modes?.availableModes.find(
            (mode) => isDefaultAgentModeValue(mode.id) || isDefaultAgentModeValue(mode.name),
          )
          ?? sessionSnapshot?.modes?.availableModes.find((mode) => mode.id !== planMode.id);
      if (!target || !sessionKey) return;
      const targetSessionKey = sessionKey;
      const updateId = 'session-mode';
      setConfigUpdatingBySession((current) => ({
        ...current,
        [targetSessionKey]: updateId,
      }));
      try {
        await setSessionMode(targetSessionKey, target.id);
      } catch (error) {
        if (sessionKeyRef.current === targetSessionKey) {
          messageApi.error(String(error));
        }
      } finally {
        setConfigUpdatingBySession((current) => {
          if (current[targetSessionKey] !== updateId) return current;
          const { [targetSessionKey]: _completed, ...remaining } = current;
          return remaining;
        });
      }
      return;
    }
    if (planOption) {
      const currentlyOn = isPlanModeValue(planOption.currentValue);
      if (enabled === currentlyOn) return;
      const choices = configChoices(planOption);
      const target = enabled
        ? choices.find((choice) => isPlanModeValue(choice.value))
        : choices.find((choice) => isDefaultAgentModeValue(choice.value))
          ?? choices.find((choice) => !isPlanModeValue(choice.value));
      if (!target) return;
      await applyConfigChoice(planOption.id, target.value);
    }
  }, [
    applyConfigChoice,
    configUpdatingId,
    messageApi,
    planMode,
    planModeToggleDisabled,
    planOption,
    sessionKey,
    sessionSnapshot?.modes?.availableModes,
    sessionSnapshot?.modes?.currentModeId,
    setSessionMode,
  ]);

  const togglePlanMode = useCallback(async () => {
    await setPlanModeEnabled(!planEnabled);
  }, [planEnabled, setPlanModeEnabled]);

  const disablePlanMode = useCallback(async () => {
    if (!planEnabled) return;
    await setPlanModeEnabled(false);
  }, [planEnabled, setPlanModeEnabled]);

  const agentMenuItems = useMemo<MenuProps['items']>(
    () =>
      agents.map((a) => ({
        key: a.id,
        // Icon lives inside the label so spacing is reliable (antd item-icon margin varies by theme).
        label: (
          <span
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 8,
              minWidth: 0,
            }}
          >
            <AcpAgentIcon agentId={a.id} agentName={a.name} icon={a.icon} size={16} />
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{a.name}</span>
          </span>
        ),
        disabled: !!activeThreadId, // thread is bound to one agent for life
      })),
    [agents, activeThreadId],
  );

  const projectMenuItems = useMemo<MenuProps['items']>(
    () =>
      projects.filter((project) => project.kind === 'project').map((p) => ({
        key: p.id,
        label: (
          <span
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 8,
              minWidth: 0,
              maxWidth: 280,
            }}
          >
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</span>
          </span>
        ),
      })),
    [projects],
  );

  /** Shared dashed-underline chip for welcome title agent / project pickers. */
  const welcomeLinkStyle: CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 6,
    margin: '0 2px',
    padding: '0 2px',
    border: 'none',
    borderBottom: `1px dashed ${token.colorTextSecondary}`,
    borderRadius: 0,
    background: 'transparent',
    color: token.colorText,
    fontSize: 'inherit',
    fontWeight: 600,
    lineHeight: 1.35,
    height: 'auto',
    cursor: 'pointer',
    verticalAlign: 'baseline',
  };

  const gitBranchItems = useMemo<MenuProps['items']>(() => {
    if (!gitInfo?.isRepo || gitInfo.branches.length === 0) return [];
    return gitInfo.branches.map((b) => ({
      key: b,
      label: (
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
          {b === gitInfo.branch ? <Check size={12} /> : <span style={{ width: 12 }} />}
          <span style={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 12 }}>
            {b}
          </span>
        </span>
      ),
    }));
  }, [gitInfo]);

  const handleGitCheckout = useCallback(
    async (branch: string) => {
      if (!activeProjectId || !branch || branch === gitInfo?.branch) return;
      const projectId = activeProjectId;
      setCheckoutLoading(true);
      try {
        const info = await invoke<AcpGitInfo>('acp_git_checkout', {
          projectId,
          branch,
        });
        if (activeProjectIdRef.current !== projectId) return;
        setGitInfo(info);
        messageApi.success(t('agentPage.branchSwitched', { branch }));
      } catch (e) {
        if (activeProjectIdRef.current !== projectId) return;
        messageApi.error(String(e));
      } finally {
        if (activeProjectIdRef.current === projectId) setCheckoutLoading(false);
      }
    },
    [activeProjectId, gitInfo?.branch, messageApi, t],
  );

  const resizeTextareaToContent = useCallback(() => {
    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      if (!textarea) return;
      textarea.style.height = 'auto';
      const desired = hasUserResizedRef.current
        ? userMinHeightRef.current
        : Math.max(textarea.scrollHeight, userMinHeightRef.current);
      textarea.style.height = `${Math.min(desired, COMPOSER_ABSOLUTE_MAX_HEIGHT)}px`;
    });
  }, []);

  useEffect(() => {
    if (!composerRecoveryId) return;
    const recovery = takeComposerRecovery(currentComposerScopeKey);
    if (!recovery) return;
    const nextValue = mergeComposerRecoveryText(valueRef.current, recovery.text);
    valueRef.current = nextValue;
    setValue(nextValue);
    resizeTextareaToContent();
    messageApi.error(recovery.error);
  }, [
    composerRecoveryId,
    currentComposerScopeKey,
    messageApi,
    resizeTextareaToContent,
    takeComposerRecovery,
  ]);

  const removeSnippet = useCallback((id: string) => {
    setPastedSnippets((previous) => {
      const target = previous.find((snippet) => snippet.id === id);
      if (!target) return previous;
      setValue((current) => removePasteTokens(current, target.index));
      resizeTextareaToContent();
      return previous.filter((snippet) => snippet.id !== id);
    });
  }, [resizeTextareaToContent]);

  const handlePaste = useCallback((event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    if (handleClipboardFiles(event)) return;
    const text = event.clipboardData?.getData('text/plain');
    if (!text || !isLongPastedText(text)) return;
    event.preventDefault();
    pastedSnippetSeqRef.current += 1;
    const index = pastedSnippetSeqRef.current;
    setPastedSnippets((previous) => [...previous, createPastedSnippet(text, index)]);

    const textarea = event.currentTarget;
    const start = textarea.selectionStart ?? value.length;
    const end = textarea.selectionEnd ?? start;
    const inserted = insertPasteTokenAtSelection(value, start, end, index);
    setValue(inserted.value);
    requestAnimationFrame(() => {
      const current = textareaRef.current;
      if (!current) return;
      current.focus();
      current.setSelectionRange(inserted.caret, inserted.caret);
      current.style.height = 'auto';
      current.style.height = `${Math.min(
        Math.max(current.scrollHeight, userMinHeightRef.current),
        COMPOSER_ABSOLUTE_MAX_HEIGHT,
      )}px`;
    });
  }, [handleClipboardFiles, value]);

  const handleSend = async () => {
    const submittedValue = value;
    const submittedSnippets = pastedSnippets;
    const submittedScopeKey = currentComposerScopeKey;
    let recoveryScopeKey = submittedScopeKey;
    const mergedContent = mergePastedSnippetsIntoContent(submittedValue, submittedSnippets);
    if (
      (!mergedContent && attachedFiles.length === 0)
      || sending
      || streaming
      || preparing
      || configUpdatingId
      || messagesLoading
      || !!messagesError
      || !sessionSnapshot
    ) return;
    if (!effectiveAgentId) {
      messageApi.warning(t('agentPage.noAgents'));
      return;
    }
    if (!supportsImageAttachments && attachedFiles.some(({ file }) => isImageFile(file))) {
      messageApi.warning(t('agentPage.imageAttachmentUnsupported'));
      return;
    }

    const submittedAttachments = detachAttachments();
    setSending(true);
    useAcpStore.setState({ composerSubmitting: true });
    valueRef.current = '';
    pastedSnippetsRef.current = [];
    setValue('');
    setPastedSnippets([]);
    pastedSnippetSeqRef.current = 0;
    if (textareaRef.current) {
      textareaRef.current.style.height = hasUserResizedRef.current
        ? `${userMinHeightRef.current}px`
        : 'auto';
    }
    setStickToBottomState(true);
    setShowScrollToBottom(false);
    try {
      const attachmentInputs = submittedAttachments.length > 0
        ? await Promise.all(
            submittedAttachments.map(({ file }) => fileToAttachmentInput(file)),
          )
        : undefined;
      const finalContent = mergedContent
        || t('chat.attachmentOnlyMessage');
      const titleSeed = submittedValue.replace(/\[\[paste:#\d+\]\]/g, '').trim()
        || submittedSnippets[0]?.content.slice(0, 80)
        || submittedAttachments[0]?.file.name
        || t('agentPage.newThread');

      let threadId = activeThreadId;
      if (!threadId) {
        const projectId = activeProjectId ?? (await ensureRecentDraft()).id;
        const pendingDraftKey = `draft:${projectId}:${effectiveAgentId}`;
        // The effect above normally finishes this work while the user types.
        // Only await preparation when no authoritative draft snapshot exists;
        // otherwise a second IPC delays the first visible message for no gain.
        if (!useAcpStore.getState().sessionByThread[pendingDraftKey]) {
          await prepareDraft(projectId, effectiveAgentId);
        }
        // First message in project → create thread then send. A hidden Recent
        // draft follows this same adoption path as a regular project draft.
        const thread = await createThread(
          projectId,
          effectiveAgentId,
          titleSeed.slice(0, 48),
        );
        threadId = thread.id;
        recoveryScopeKey = `${projectId}:${thread.id}`;
        // Draft adoption is the one scope transition that belongs to this send.
        // Mark it consumed so a delayed effect cannot erase a failed-send restore.
        previousComposerScopeRef.current = recoveryScopeKey;
      }
      await sendPrompt(threadId, finalContent, attachmentInputs);
      revokeComposerAttachments(submittedAttachments);
    } catch (e) {
      const currentScopeKey = composerScopeRef.current;
      const currentStore = useAcpStore.getState();
      const currentProject = currentStore.projects.find(
        (project) => project.id === currentStore.activeProjectId,
      ) ?? null;
      const currentStoreScopeKey = composerScopeKey(
        currentProject,
        currentStore.activeThreadId,
      );
      const belongsToSubmission = (scopeKey: string) => scopeKey === submittedScopeKey
        || scopeKey === recoveryScopeKey;
      const canRestore = belongsToSubmission(currentScopeKey)
        && belongsToSubmission(currentStoreScopeKey);
      if (canRestore) {
        setValue((current) => current || submittedValue);
        restoreAttachments(submittedAttachments);
        setPastedSnippets((current) => (
          current.length > 0 ? current : submittedSnippets
        ));
        resizeTextareaToContent();
      } else {
        revokeComposerAttachments(submittedAttachments);
      }
      messageApi.error(String(e));
    } finally {
      setSending(false);
      useAcpStore.setState({ composerSubmitting: false });
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Shift+Tab toggles plan mode (Codex-style), when the agent advertises plan.
    if (e.key === 'Tab' && e.shiftKey) {
      if ((planOption || planMode) && !planModeToggleDisabled) {
        e.preventDefault();
        void togglePlanMode();
      }
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void handleSend();
    }
  };

  const autoResizeTextarea = useCallback((el: HTMLTextAreaElement) => {
    el.style.height = 'auto';
    const desired = hasUserResizedRef.current
      ? userMinHeightRef.current
      : Math.max(el.scrollHeight, userMinHeightRef.current);
    el.style.height = `${Math.min(desired, COMPOSER_ABSOLUTE_MAX_HEIGHT)}px`;
  }, []);

  const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setValue(e.target.value);
    autoResizeTextarea(e.target);
  };

  const handleResizeMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    resizeCleanupRef.current();
    const textarea = textareaRef.current;
    const startHeight = textarea ? textarea.offsetHeight : userMinHeightRef.current;
    dragStateRef.current = { startY: e.clientY, startH: startHeight };
    const onMouseMove = (ev: MouseEvent) => {
      if (!dragStateRef.current) return;
      const delta = dragStateRef.current.startY - ev.clientY;
      const newH = Math.max(
        COMPOSER_INITIAL_MIN_HEIGHT,
        Math.min(COMPOSER_ABSOLUTE_MAX_HEIGHT, dragStateRef.current.startH + delta),
      );
      hasUserResizedRef.current = true;
      setUserMinHeight(newH);
      userMinHeightRef.current = newH;
      if (textarea) {
        textarea.style.height = `${newH}px`;
      }
    };
    const cleanupResize = () => {
      dragStateRef.current = null;
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', cleanupResize);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      resizeCleanupRef.current = () => {};
    };
    resizeCleanupRef.current = cleanupResize;
    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', cleanupResize);
    document.body.style.cursor = 'ns-resize';
    document.body.style.userSelect = 'none';
  }, []);

  useEffect(() => () => {
    resizeCleanupRef.current();
  }, []);

  const handleCancel = useCallback(async () => {
    if (!activeThreadId || cancelling) return;
    try {
      await cancelPrompt(activeThreadId);
    } catch (error) {
      messageApi.error(String(error));
    }
  }, [activeThreadId, cancelPrompt, cancelling, messageApi]);

  const setStickToBottomState = useCallback((next: boolean) => {
    stickToBottomRef.current = next;
  }, []);

  const getScrollBox = useCallback((): HTMLElement | null => {
    return (bubbleListRef.current?.scrollBoxNativeElement as HTMLElement | null | undefined) ?? null;
  }, []);

  const syncScrollToBottomVisibility = useCallback(() => {
    const target = getScrollBox();
    if (!target) return;
    const next = shouldShowScrollToBottom(
      target.scrollHeight,
      target.scrollTop,
      target.clientHeight,
      CHAT_SCROLL_IS_REVERSED,
    );
    setShowScrollToBottom((prev) => (prev === next ? prev : next));
  }, [getScrollBox]);

  const scrollListToBottom = useCallback((behavior: ScrollBehavior = 'auto') => {
    try {
      bubbleListRef.current?.scrollTo({ top: 'bottom', behavior });
    } catch {
      // jsdom (and some hosts) may not implement Element.scrollTo
    }
    setShowScrollToBottom(false);
    setStickToBottomState(true);
  }, [setStickToBottomState]);

  const handleBubbleListScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    const target = event.currentTarget;
    setShowScrollToBottom(
      shouldShowScrollToBottom(
        target.scrollHeight,
        target.scrollTop,
        target.clientHeight,
        CHAT_SCROLL_IS_REVERSED,
      ),
    );
    const keepAutoScroll = shouldKeepAutoScroll(
      target.scrollHeight,
      target.scrollTop,
      target.clientHeight,
      CHAT_SCROLL_IS_REVERSED,
      CHAT_AUTO_SCROLL_BOTTOM_THRESHOLD,
    );
    if (keepAutoScroll !== stickToBottomRef.current) {
      setStickToBottomState(keepAutoScroll);
    }
  }, [setStickToBottomState]);

  // Reset stick-to-bottom when switching threads
  useEffect(() => {
    setShowScrollToBottom(false);
    setStickToBottomState(true);
    const id = window.requestAnimationFrame(() => {
      if (!stickToBottomRef.current) return;
      try {
        bubbleListRef.current?.scrollTo({ top: 'bottom', behavior: 'auto' });
      } catch {
        // ignore
      }
    });
    return () => window.cancelAnimationFrame(id);
  }, [activeThreadId, setStickToBottomState]);

  // Active: streaming start / end while sticking → scroll to bottom
  const prevStreamingRef = useRef(false);
  useEffect(() => {
    let timeoutId: number | null = null;
    if (streaming && !prevStreamingRef.current) {
      timeoutId = window.setTimeout(() => scrollListToBottom('smooth'), 50);
    } else if (!streaming && prevStreamingRef.current && stickToBottomRef.current) {
      timeoutId = window.setTimeout(() => {
        try {
          bubbleListRef.current?.scrollTo({ top: 'bottom', behavior: 'auto' });
        } catch {
          // ignore
        }
        syncScrollToBottomVisibility();
      }, 30);
    }
    prevStreamingRef.current = streaming;
    return () => {
      if (timeoutId !== null) window.clearTimeout(timeoutId);
    };
  }, [scrollListToBottom, streaming, syncScrollToBottomVisibility]);

  // Passive: follow message growth while stick-to-bottom (streaming tokens / new bubbles)
  useEffect(() => {
    if (!stickToBottomRef.current) {
      syncScrollToBottomVisibility();
      return;
    }
    const id = window.requestAnimationFrame(() => {
      if (!stickToBottomRef.current) return;
      try {
        bubbleListRef.current?.scrollTo({ top: 'bottom', behavior: 'auto' });
      } catch {
        // ignore
      }
    });
    return () => window.cancelAnimationFrame(id);
  }, [messages, streaming, syncScrollToBottomVisibility]);

  // Follow layout growth (markdown / tool cards expanding) while stick-to-bottom
  useEffect(() => {
    if (typeof ResizeObserver === 'undefined') return;
    let frameId = 0;
    const scrollBox = getScrollBox();
    const scrollContent = scrollBox?.querySelector(
      '.ant-bubble-list-scroll-content',
    ) as HTMLElement | null;
    if (!scrollBox || !scrollContent) return;

    const observer = new ResizeObserver(() => {
      if (frameId) window.cancelAnimationFrame(frameId);
      frameId = window.requestAnimationFrame(() => {
        if (stickToBottomRef.current) {
          try {
            bubbleListRef.current?.scrollTo({ top: 'bottom', behavior: 'auto' });
          } catch {
            // ignore
          }
        } else {
          syncScrollToBottomVisibility();
        }
      });
    });
    observer.observe(scrollContent);
    return () => {
      if (frameId) window.cancelAnimationFrame(frameId);
      observer.disconnect();
    };
  }, [activeThreadId, getScrollBox, messages.length, syncScrollToBottomVisibility]);

  const canSend =
    !sending
    && !streaming
    && !preparing
    && !configUpdatingId
    && !messagesLoading
    && !messagesError
    && !!sessionSnapshot
    && (value.trim().length > 0 || attachedFiles.length > 0 || pastedSnippets.length > 0)
    && !!effectiveAgentId;

  // Codex-style project welcome prompts
  const projectPromptItems: PromptsItemType[] = useMemo(
    () => [
      {
        key: 'explore',
        icon: <Telescope size={16} style={{ color: '#3b82f6' }} />,
        label: t('agentPage.promptExplore'),
      },
      {
        key: 'build',
        icon: <Hammer size={16} style={{ color: '#a855f7' }} />,
        label: t('agentPage.promptBuild'),
      },
      {
        key: 'review',
        icon: <RefreshCw size={16} style={{ color: '#22c55e' }} />,
        label: t('agentPage.promptReview'),
      },
      {
        key: 'fix',
        icon: <Bug size={16} style={{ color: '#f97316' }} />,
        label: t('agentPage.promptFix'),
      },
    ],
    [t],
  );

  const handleProjectPromptClick = useCallback(
    (info: { data: PromptsItemType }) => {
      const text = typeof info.data.label === 'string' ? info.data.label : '';
      if (!text) return;
      setValue(text);
      // Focus input so user can edit / send
      requestAnimationFrame(() => textareaRef.current?.focus());
    },
    [],
  );

  const showMessageLoadState = !!(
    activeThread
    && messages.length === 0
    && (messagesLoading || messagesError)
  );
  const showMessageLoadErrorBanner = !!(
    activeThread
    && messages.length > 0
    && messagesError
  );
  const showProjectEmpty = !activeThread
    || (messages.length === 0 && !showMessageLoadState);

  const activeModelChoice = modelOption
    ? configChoices(modelOption).find(
      (c) => String(c.value) === String(modelOption.currentValue),
    )
    : undefined;

  const thoughtIsMax = isMaxThoughtLevel(thoughtOption);
  const thoughtAccent = thoughtIsMax ? '#7c3aed' : undefined;

  const renderConfigDropdown = (
    option: AcpSessionConfigOption,
    opts?: {
      icon?: ReactNode;
      showModelIcon?: boolean;
      accentColor?: string;
    },
  ) => (
    <Dropdown
      key={option.id}
      menu={{
        items: choiceItems(option),
        selectedKeys: [String(option.currentValue)],
        onClick: ({ key }) => {
          void applyConfigChoice(option.id, configChoicePayload(option, key));
        },
        style: { maxHeight: 360, overflowY: 'auto' },
      }}
      trigger={['click']}
      placement="topRight"
      disabled={sending || streaming || preparing || !!configUpdatingId}
    >
      <Button
        type="text"
        size="small"
        loading={configUpdatingId === option.id}
        aria-label={`${
          option === modelOption
            ? t('agentPage.model')
            : option === thoughtOption
              ? t('agentPage.reasoning')
              : option === permissionOption
                ? t('agentPage.interactionPermissionTitle')
                : option.name
        }: ${selectedOptionLabel(option)}`}
        icon={
          opts?.showModelIcon ? (
            <AcpModelChoiceIcon
              modelId={
                activeModelChoice
                  ? modelIconKey(activeModelChoice)
                  : String(option.currentValue ?? 'model')
              }
              agentId={effectiveAgentId}
              agentName={agentMeta?.name}
              agentIcon={agentMeta?.icon}
              size={14}
            />
          ) : opts?.icon
        }
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 4,
          fontSize: 12,
          color: opts?.accentColor,
        }}
      >
        {selectedOptionLabel(option)}
      </Button>
    </Dropdown>
  );

  const renderSpeedToggle = (option: AcpSessionConfigOption) => {
    const enabled = isSpeedEnabled(option);
    const label = option.name || t('agentPage.fast');
    const tip = enabled
      ? t('agentPage.fastOnTip', { name: label })
      : t('agentPage.fastOffTip', { name: label });
    return (
      <Tooltip key={option.id} title={tip}>
        <Button
          type="text"
          size="small"
          loading={configUpdatingId === option.id}
          disabled={sending || streaming || preparing || !!configUpdatingId}
          aria-pressed={enabled}
          aria-label={label}
          onClick={() => {
            const next = nextSpeedValue(option);
            void applyConfigChoice(
              option.id,
              typeof next === 'boolean' ? next : next,
            );
          }}
          icon={(
            <Zap
              size={14}
              fill={enabled ? 'currentColor' : 'none'}
              style={{ color: enabled ? token.colorWarning : token.colorTextSecondary }}
            />
          )}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            color: enabled ? token.colorWarning : token.colorTextSecondary,
          }}
        />
      </Tooltip>
    );
  };

  const renderPlanProgressControl = () => (
    streaming && activePlan && activePlan.total > 0 ? (
      <Popover
        placement="topRight"
        title={t('agentPage.planProgress')}
        content={(
          <div style={{ width: 280, maxHeight: 260, overflowY: 'auto' }}>
            {activePlan.entries.map((entry, index) => (
              <div
                key={`${index}-${entry.content}`}
                style={{ display: 'flex', gap: 8, paddingBlock: 4, fontSize: 12 }}
              >
                <span>{['completed', 'complete', 'done'].includes(entry.status) ? '✓' : '○'}</span>
                <span style={{ flex: 1 }}>{entry.content}</span>
              </div>
            ))}
          </div>
        )}
      >
        <Button type="text" size="small" style={{ display: 'flex', gap: 5 }}>
          <Progress
            type="circle"
            size={16}
            showInfo={false}
            percent={(activePlan.completed / activePlan.total) * 100}
          />
          <ListTodo size={14} />
          {activePlan.completed}/{activePlan.total}
        </Button>
      </Popover>
    ) : null
  );

  const renderStopControl = () => (
    streaming ? (
      <Button
        shape="circle"
        size="small"
        danger
        icon={<Square size={14} />}
        onClick={() => void handleCancel()}
        loading={cancelling}
        aria-label={t('agentPage.stop')}
      />
    ) : null
  );

  const renderComposer = () => (
    <div className="relative px-4 pb-3 pt-1 shrink-0" {...dragHandlers}>
      <input
        ref={fileInputRef}
        type="file"
        multiple
        style={{ display: 'none' }}
        onChange={handleFileChange}
      />
      <AttachmentChips
        attachments={attachedFiles}
        snippets={pastedSnippets}
        onRemoveAttachment={removeAttachment}
        onRemoveSnippet={removeSnippet}
      />
      {showScrollToBottom && !showProjectEmpty ? (
        <Button
          size="small"
          shape="round"
          icon={<ChevronDown size={14} />}
          onClick={() => scrollListToBottom('smooth')}
          style={{
            position: 'absolute',
            left: '50%',
            top: -28,
            zIndex: 2,
            transform: 'translateX(-50%)',
            boxShadow: token.boxShadowSecondary,
          }}
        >
          {t('chat.scrollToBottom')}
        </Button>
      ) : null}
      <div
        style={{
          border: '1px solid var(--border-color)',
          borderRadius: 16,
          backgroundColor: token.colorBgContainer,
          overflow: 'hidden',
        }}
      >
        {activeInteraction ? (
          <div
            style={{
              display: 'flex',
              minWidth: 0,
              maxHeight: 'min(60vh, 520px)',
              flexDirection: 'column',
              gap: 8,
              padding: 14,
              overflow: 'hidden',
            }}
          >
            {pendingInteractions.length > 1 ? (
              <div
                style={{
                  display: 'flex',
                  flexShrink: 0,
                  alignItems: 'center',
                  justifyContent: 'flex-end',
                  gap: 4,
                }}
              >
                <Button
                  type="text"
                  size="small"
                  disabled={clampedInteractionIndex <= 0}
                  icon={<ChevronLeft size={16} />}
                  aria-label={t('agentPage.interactionPrevItem')}
                  onClick={() => {
                    const previous = pendingInteractions[clampedInteractionIndex - 1];
                    if (previous) setActiveInteractionId(previous.requestId);
                  }}
                />
                <Text
                  type="secondary"
                  aria-live="polite"
                  style={{
                    minWidth: 44,
                    textAlign: 'center',
                    fontSize: 12,
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {clampedInteractionIndex + 1}/{pendingInteractions.length}
                </Text>
                <Button
                  type="text"
                  size="small"
                  disabled={clampedInteractionIndex >= pendingInteractions.length - 1}
                  icon={<ChevronRight size={16} />}
                  aria-label={t('agentPage.interactionNextItem')}
                  onClick={() => {
                    const next = pendingInteractions[clampedInteractionIndex + 1];
                    if (next) setActiveInteractionId(next.requestId);
                  }}
                />
              </div>
            ) : null}
            <div style={{ minWidth: 0, minHeight: 0, flex: 1, overflow: 'hidden' }}>
              {pendingInteractions.map((interaction, index) => {
                const isActive = index === clampedInteractionIndex;
                return (
                  <div
                    key={interaction.requestId}
                    hidden={!isActive}
                    aria-hidden={!isActive || undefined}
                    style={isActive ? { minWidth: 0, minHeight: 0, height: '100%' } : undefined}
                  >
                    <AcpInteractionComposer
                      active={isActive}
                      request={interaction}
                      onSubmit={(submission) => (
                        'optionId' in submission
                          ? respondPermission(
                              interaction.requestId,
                              submission.optionId,
                              submission.feedback,
                            )
                          : 'outcome' in submission
                            ? cancelInteraction(interaction.requestId)
                          : respondQuestionnaire(
                              interaction.requestId,
                              submission.questionnaire,
                            )
                      )}
                    />
                  </div>
                );
              })}
            </div>
            <div className="flex items-center justify-end gap-1" style={{ flexShrink: 0 }}>
              {renderPlanProgressControl()}
              {renderStopControl()}
            </div>
          </div>
        ) : (
          <>
        {/* Drag-to-resize handle (parity with chat InputArea) */}
        <div
          onMouseDown={handleResizeMouseDown}
          style={{
            height: 10,
            cursor: 'ns-resize',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0,
          }}
        >
          <GripHorizontal size={14} style={{ color: token.colorTextQuaternary, opacity: 0.5 }} />
        </div>
        <textarea
          className="aqbot-input-textarea"
          ref={textareaRef}
          value={value}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          placeholder={t('agentPage.inputPlaceholder')}
          aria-label={t('agentPage.inputPlaceholder')}
          name="acp-prompt"
          autoComplete="off"
          rows={1}
          disabled={sending}
          style={{
            width: '100%',
            border: 'none',
            outline: 'none',
            resize: 'none',
            padding: '8px 16px 8px',
            fontSize: token.fontSize,
            lineHeight: 1.6,
            backgroundColor: 'transparent',
            color: token.colorText,
            fontFamily: 'inherit',
            minHeight: userMinHeight,
            maxHeight: COMPOSER_ABSOLUTE_MAX_HEIGHT,
            height: hasUserResizedRef.current ? userMinHeight : undefined,
            overflowY: 'auto',
          }}
        />
        {/* Inside input: permission (left) · model/reasoning/fast + send (right) */}
        <div className="flex flex-wrap items-center justify-between gap-1 px-2 pb-2">
          <div className="flex flex-wrap items-center gap-0.5">
            <Tooltip title={t('agentPage.attachFile')}>
              <Button
                type="text"
                size="small"
                icon={<Paperclip size={14} />}
                onClick={openFilePicker}
                disabled={sending || streaming || !effectiveAgentId}
                aria-label={t('agentPage.attachFile')}
              />
            </Tooltip>
            {permissionOption ? (
              <Dropdown
                menu={{
                  items: choiceItems(permissionOption, permissionChoices),
                  selectedKeys: selectedPermissionValue == null
                    ? []
                    : [String(selectedPermissionValue)],
                  onClick: ({ key }) => handlePermissionChange(key),
                }}
                trigger={['click']}
                placement="topLeft"
                disabled={sending || planEnabled || streaming || preparing || !!configUpdatingId}
              >
                <Button
                  type="text"
                  size="small"
                  loading={preparing || configUpdatingId === permissionOption.id}
                  aria-label={`${t('agentPage.interactionPermissionTitle')}: ${
                    selectedPermissionLabel
                  }`}
                  icon={isFullAccessPermissionChoice(
                    selectedPermissionValue,
                    selectedPermissionLabel,
                  )
                    ? <ShieldAlert size={14} style={{ color: '#ff4d4f' }} />
                    : /agent|auto|acceptedits/i.test(String(selectedPermissionValue))
                      ? <ShieldCheck size={14} style={{ color: '#1890ff' }} />
                      : <Shield size={14} />}
                  style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 12 }}
                >
                  {selectedPermissionLabel}
                </Button>
              </Dropdown>
            ) : null}
          </div>
          <div className="flex items-center gap-1 ml-auto">
            {modelOption ? renderConfigDropdown(modelOption, { showModelIcon: true }) : null}
            {thoughtOption ? renderConfigDropdown(thoughtOption, {
              icon: (
                <BrainCircuit
                  size={14}
                  style={thoughtAccent ? { color: thoughtAccent } : undefined}
                />
              ),
              accentColor: thoughtAccent,
            }) : null}
            {modelConfigExtras.map((option) => renderSpeedToggle(option))}
            {renderPlanProgressControl()}
            {streaming ? (
              renderStopControl()
            ) : (
              <Button
                type="primary"
                shape="circle"
                size="small"
                icon={<ArrowUp size={14} />}
                onClick={() => void handleSend()}
                disabled={!canSend}
                loading={sending}
                aria-label={t('agentPage.send')}
              />
            )}
          </div>
        </div>
          </>
        )}
      </div>

      {/* Bottom bar: agent picker + plan tag · git branch */}
      <div className="flex flex-wrap items-center justify-between gap-y-1 px-1 pt-1">
        <div className="flex flex-wrap items-center gap-1">
          {!sessionKey && recentDraftError ? (
            <Tooltip title={recentDraftError}>
              <Button
                type="text"
                danger
                size="small"
                icon={<RefreshCw size={14} />}
                onClick={() => void prepareRecentWorkspace()}
                style={{ fontSize: 12 }}
              >
                {t('agentPage.retryConnection')}
              </Button>
            </Tooltip>
          ) : !sessionKey && recentDraftPreparing ? (
            <Button type="text" size="small" loading disabled style={{ fontSize: 12 }}>
              {agentProcessReady
                ? t('agentPage.preparingConversation')
                : t('agentPage.preparing')}
            </Button>
          ) : sessionKey && !sessionSnapshot ? (
            preparing || !statusByThread[sessionKey] ? (
              <Button type="text" size="small" loading disabled style={{ fontSize: 12 }}>
                {agentProcessReady
                  ? t('agentPage.preparingConversation')
                  : t('agentPage.preparing')}
              </Button>
            ) : (
              <Tooltip
                title={localizedStatus(statusByThread[sessionKey])
                  || t('agentPage.prepareFailed')}
              >
                <Button
                  type="text"
                  danger
                  size="small"
                  icon={<RefreshCw size={14} />}
                  onClick={() => {
                    if (activeThreadId) {
                      void prepareSession(activeThreadId).catch(() => undefined);
                    } else if (activeProjectId && effectiveAgentId) {
                      void prepareDraft(activeProjectId, effectiveAgentId).catch(() => undefined);
                    }
                  }}
                  style={{ fontSize: 12 }}
                >
                  {t('agentPage.retryConnection')}
                </Button>
              </Tooltip>
            )
          ) : null}
          <Dropdown
            menu={{
              items: agentMenuItems,
              selectedKeys: effectiveAgentId ? [effectiveAgentId] : [],
              onClick: ({ key }) => {
                if (!activeThreadId) {
                  setComposerAgentId(key);
                }
              },
            }}
            trigger={['click']}
            disabled={sending || !!activeThreadId || agents.length === 0}
          >
            <Button
              type="text"
              size="small"
              style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12 }}
            >
              {effectiveAgentId ? (
                <AcpAgentIcon
                  agentId={effectiveAgentId}
                  agentName={agentMeta?.name}
                  icon={agentMeta?.icon}
                  size={16}
                />
              ) : (
                <Bot size={14} />
              )}
              {agentMeta?.name || t('agentPage.selectAgent')}
            </Button>
          </Dropdown>
          {planEnabled ? (
            <span
              aria-label={t('agentPage.plan')}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 4,
                height: 24,
                paddingInline: 8,
                borderRadius: token.borderRadiusSM,
                border: `1px solid ${token.colorBorder}`,
                background: token.colorBgContainer,
                color: token.colorText,
                fontSize: 12,
                lineHeight: 1,
                boxSizing: 'border-box',
              }}
            >
              <ListTodo size={12} style={{ flexShrink: 0 }} />
              <span>{t('agentPage.plan')}</span>
              <button
                type="button"
                aria-label={t('common.close')}
                onClick={() => void disablePlanMode()}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  margin: 0,
                  marginInlineStart: 2,
                  padding: 0,
                  border: 'none',
                  background: 'transparent',
                  color: 'inherit',
                  opacity: 0.55,
                  cursor: 'pointer',
                  lineHeight: 0,
                }}
              >
                <X size={12} />
              </button>
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-2 ml-auto">
          {gitInfo?.isRepo ? (
            <Dropdown
              menu={{
                items: gitBranchItems,
                selectedKeys: gitInfo.branch ? [gitInfo.branch] : [],
                onClick: ({ key }) => void handleGitCheckout(key),
                style: { maxHeight: 320, overflowY: 'auto' },
              }}
              trigger={['click']}
              placement="topRight"
              disabled={checkoutLoading || gitLoading}
            >
              <Button
                type="text"
                size="small"
                loading={checkoutLoading || gitLoading}
                icon={<GitBranch size={14} />}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4,
                  fontSize: 12,
                  maxWidth: 220,
                }}
              >
                <span
                  style={{
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
                  }}
                >
                  {gitInfo.branch || t('agentPage.detachedHead')}
                </span>
              </Button>
            </Dropdown>
          ) : activeProjectId ? (
            <Tooltip title={t('agentPage.notAGitRepo')}>
              <Button
                type="text"
                size="small"
                disabled
                icon={<GitBranch size={14} />}
                style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 12 }}
              >
                {t('agentPage.noGit')}
              </Button>
            </Tooltip>
          ) : null}
        </div>
      </div>
      {isDragging ? (
        <div
          className="fixed inset-0 z-[1000] flex items-center justify-center"
          style={{
            background: 'rgba(0, 0, 0, 0.36)',
            backdropFilter: 'blur(2px)',
            pointerEvents: 'none',
          }}
        >
          <div
            className="flex flex-col items-center gap-3 rounded-2xl px-10 py-8"
            style={{
              color: token.colorTextLightSolid,
              border: '2px dashed currentColor',
            }}
          >
            <Upload size={34} />
            <Text style={{ color: 'inherit' }}>{t('agentPage.dropFiles')}</Text>
          </div>
        </div>
      ) : null}
    </div>
  );

  return (
    <div
      className="flex flex-col h-full min-w-0"
      style={{ backgroundColor: token.colorBgElevated, overflow: 'hidden' }}
    >
      {/* Message / welcome area */}
      <div style={{ flex: 1, minHeight: 0, position: 'relative' }}>
        {showMessageLoadState ? (
          <div
            role="status"
            aria-live="polite"
            style={{
              position: 'absolute',
              inset: 0,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 12,
              padding: 24,
              textAlign: 'center',
            }}
          >
            {messagesLoading ? (
              <Button type="text" loading disabled>
                {t('common.loading')}
              </Button>
            ) : (
              <>
                <Text type="danger">{t('error.loadFailed')}</Text>
                <Text
                  type="secondary"
                  style={{ maxWidth: 560, overflowWrap: 'anywhere' }}
                >
                  {messagesError}
                </Text>
                <Button
                  icon={<RefreshCw size={14} />}
                  onClick={() => {
                    if (activeThreadId) void loadMessages(activeThreadId);
                  }}
                >
                  {t('agentPage.retryConnection')}
                </Button>
              </>
            )}
          </div>
        ) : showProjectEmpty ? (
          <div
            style={{
              position: 'absolute',
              inset: 0,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              overflow: 'auto',
              padding: 24,
            }}
          >
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'center',
                textAlign: 'center',
                gap: 28,
                width: '100%',
                maxWidth: 720,
                margin: 'auto',
              }}
            >
              <Title
                level={3}
                style={{
                  margin: 0,
                  fontWeight: 500,
                  textAlign: 'center',
                  fontSize: 22,
                  width: '100%',
                  lineHeight: 1.55,
                }}
              >
                {/* 让 {Agent} 在 {project} 中开发什么 — agent/project are dashed dropdowns */}
                {t('agentPage.projectWelcomePrefix')}
                {' '}
                <Dropdown
                  menu={{
                    items: agentMenuItems,
                    selectedKeys: effectiveAgentId ? [effectiveAgentId] : [],
                    onClick: ({ key }) => {
                      if (!activeThreadId) setComposerAgentId(key);
                    },
                  }}
                  trigger={['click']}
                  disabled={sending || agents.length === 0}
                >
                  <button type="button" style={welcomeLinkStyle}>
                    {effectiveAgentId ? (
                      <AcpAgentIcon
                        agentId={effectiveAgentId}
                        agentName={agentMeta?.name}
                        icon={agentMeta?.icon}
                        size={20}
                      />
                    ) : (
                      <Bot size={18} />
                    )}
                    <span>
                      {agentMeta?.name || t('agentPage.selectAgent')}
                    </span>
                  </button>
                </Dropdown>
                {activeProject?.kind === 'project' ? (
                  <>
                    {' '}
                    {t('agentPage.projectWelcomeMiddle')}
                    {' '}
                    <Dropdown
                      menu={{
                        items: projectMenuItems,
                        selectedKeys: activeProjectId ? [activeProjectId] : [],
                        onClick: ({ key }) => {
                          void selectProject(key);
                        },
                      }}
                      trigger={['click']}
                    >
                      <button type="button" style={welcomeLinkStyle}>
                        <span>{activeProject.name}</span>
                      </button>
                    </Dropdown>
                    {' '}
                    {t('agentPage.projectWelcomeSuffix')}
                  </>
                ) : (
                  <>
                    {' '}
                    {t('agentPage.recentWelcomeSuffix')}
                  </>
                )}
              </Title>
              <Prompts
                items={projectPromptItems}
                onItemClick={handleProjectPromptClick}
                wrap
                styles={{
                  list: {
                    justifyContent: 'center',
                    width: '100%',
                  },
                  item: {
                    // keep chips readable when wrapping
                  },
                }}
                style={{
                  marginTop: 4,
                  width: '100%',
                  display: 'flex',
                  justifyContent: 'center',
                }}
              />
            </div>
          </div>
        ) : (
          <>
            {showMessageLoadErrorBanner ? (
              <div
                role="alert"
                style={{
                  position: 'absolute',
                  top: 12,
                  left: 24,
                  right: 24,
                  zIndex: 2,
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  padding: '8px 10px',
                  border: `1px solid ${token.colorWarningBorder}`,
                  borderRadius: token.borderRadiusLG,
                  background: token.colorWarningBg,
                  boxShadow: token.boxShadowTertiary,
                }}
              >
                <Text type="warning" strong style={{ flexShrink: 0 }}>
                  {t('error.loadFailed')}
                </Text>
                <Text
                  type="secondary"
                  ellipsis={{ tooltip: messagesError }}
                  style={{ minWidth: 0, flex: 1 }}
                >
                  {messagesError}
                </Text>
                <Button
                  size="small"
                  icon={<RefreshCw size={14} />}
                  onClick={() => {
                    if (activeThreadId) void loadMessages(activeThreadId);
                  }}
                >
                  {t('agentPage.retryConnection')}
                </Button>
              </div>
            ) : null}
            {/* Match ChatView bubble + markstream layout constraints (code blocks, overflow) */}
            <style>{`
              .aqbot-acp-bubble-list .ant-bubble,
              .aqbot-acp-bubble-list .ant-bubble-content-wrapper,
              .aqbot-acp-bubble-list .ant-bubble-body {
                min-width: 0;
                max-width: 100%;
              }
              .aqbot-acp-bubble-list .ant-bubble-footer {
                margin-block-start: 4px !important;
              }
              .aqbot-acp-bubble-list .ant-bubble-start .ant-bubble-body {
                width: 100%;
              }
              .aqbot-acp-bubble-list .ant-bubble-content {
                overflow: hidden;
                min-width: 0;
              }
              .aqbot-acp-bubble-list .ant-bubble-content .markstream-react {
                overflow: hidden;
                min-width: 0;
              }
              .aqbot-acp-bubble-list .ant-bubble-content .code-block-node,
              .aqbot-acp-bubble-list .ant-bubble-content .code-block-container {
                overflow-x: auto;
                max-width: 100%;
                min-width: 0 !important;
                width: 100%;
                box-sizing: border-box;
              }
              /* Plan cards occupy the full message content width */
              .aqbot-acp-bubble-list .ant-bubble-content [data-type="acp-plan"],
              .aqbot-acp-bubble-list .ant-bubble-content .acp-plan-node,
              .aqbot-acp-bubble-list .ant-bubble-start .ant-bubble-content {
                max-width: 100%;
              }
              .aqbot-acp-bubble-list .ant-bubble-start.ant-bubble-role-plan .ant-bubble-content,
              .aqbot-acp-bubble-list .ant-bubble-start.ant-bubble-role-plan .ant-bubble-content-wrapper {
                width: 100%;
                max-width: 100%;
              }
              /* Match ChatView conversation bubble styles */
              .aqbot-acp-bubble-list.bubble-compact .ant-bubble {
                margin-bottom: 4px;
              }
              .aqbot-acp-bubble-list.bubble-compact .ant-bubble-content {
                padding: 6px 10px;
              }
              .aqbot-acp-bubble-list.bubble-minimal .ant-bubble-content {
                background: transparent !important;
                box-shadow: none !important;
                border: none !important;
                padding: 4px 0;
              }
            `}</style>
            <Bubble.List
              ref={bubbleListRef}
              className={`aqbot-acp-bubble-list bubble-${bubbleStyle}`}
              items={bubbleItems}
              autoScroll={false}
              onScroll={handleBubbleListScroll}
              role={roles as never}
              style={{
                height: '100%',
                padding: showMessageLoadErrorBanner
                  ? '72px 24px 16px'
                  : '16px 24px',
                overflowX: 'hidden',
              }}
            />
          </>
        )}
      </div>

      {renderComposer()}
    </div>
  );
}
