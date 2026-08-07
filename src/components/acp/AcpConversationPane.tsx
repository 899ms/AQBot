import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  App,
  Avatar,
  Button,
  Dropdown,
  Empty,
  Tag,
  Tooltip,
  Typography,
  theme,
  type MenuProps,
} from 'antd';
import Bubble from '@ant-design/x/es/bubble';
import type { BubbleItemType } from '@ant-design/x/es/bubble/interface';
import Actions from '@ant-design/x/es/actions';
import Prompts from '@ant-design/x/es/prompts';
import type { PromptsItemType } from '@ant-design/x/es/prompts';
import { setCustomComponents } from 'markstream-react';
import {
  ArrowUp,
  Bot,
  Bug,
  Check,
  Copy,
  GitBranch,
  Hammer,
  RefreshCw,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Square,
  Telescope,
  Timer,
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
import { formatChatTime } from '@/components/chat/chatTime';
import PermissionCard from '@/components/chat/PermissionCard';
import { AcpAgentIcon } from '@/lib/acpAgentIcon';
import { formatDurationI18n, parseAcpDurationMs } from '@/lib/formatDurationI18n';
import { normalizeThinkTagsForMarkdown } from '@/lib/thinkTags';
import { AcpToolCallNode } from './AcpToolCallNode';

const { Text, Title } = Typography;

// Same markstream custom tags as chat (code/links use shared CSS via aqbot-chat-markdown)
setCustomComponents('acp', {
  think: ThinkNode,
  'tool-call': AcpToolCallNode,
  image: ChatImageNode,
  img: ChatImageNode,
});

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
      `}</style>
    </span>
  );
}

interface AcpGitInfo {
  branch: string | null;
  branches: string[];
  isRepo: boolean;
}

function formatAcpTime(createdAt: string): string {
  const ms = Date.parse(createdAt.includes('T') ? createdAt : createdAt.replace(' ', 'T') + 'Z');
  if (Number.isFinite(ms)) return formatChatTime(ms);
  return createdAt.slice(11, 19) || createdAt;
}

/**
 * Main ACP conversation surface.
 *
 * - Select a project → right pane shows empty state + input (Codex-style)
 * - First send creates a thread under that project with the chosen agent
 * - Select a thread → message list + same input chrome
 */
export function AcpConversationPane() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { modal, message: messageApi } = App.useApp();
  const themeMode = useSettingsStore((s) => s.settings.theme_mode);
  const isDarkMode = useResolvedDarkMode(themeMode ?? 'system');

  const projects = useAcpStore((s) => s.projects);
  const threads = useAcpStore((s) => s.threads);
  const messages = useAcpStore((s) => s.messages);
  const activeProjectId = useAcpStore((s) => s.activeProjectId);
  const activeThreadId = useAcpStore((s) => s.activeThreadId);
  const statusByThread = useAcpStore((s) => s.statusByThread);
  const runningByThread = useAcpStore((s) => s.runningByThread);
  const permissionMode = useAcpStore((s) => s.permissionMode);
  const pendingPermissions = useAcpStore((s) => s.pendingPermissions);
  const sendPrompt = useAcpStore((s) => s.sendPrompt);
  const createThread = useAcpStore((s) => s.createThread);
  const setPermissionMode = useAcpStore((s) => s.setPermissionMode);
  const respondPermission = useAcpStore((s) => s.respondPermission);
  const enabledAgents = useAcpStore((s) => s.enabledAgents);

  const settings = useSettingsStore((s) => s.settings);
  const profile = useUserProfileStore((s) => s.profile);
  const resolvedAvatarSrc = useResolvedAvatarSrc(profile.avatarType, profile.avatarValue);
  const { copy: copyText, isCopiedFor } = useCopyToClipboard();

  const [value, setValue] = useState('');
  const [sending, setSending] = useState(false);
  const [composerAgentId, setComposerAgentId] = useState<string | null>(null);
  const [gitInfo, setGitInfo] = useState<AcpGitInfo | null>(null);
  const [gitLoading, setGitLoading] = useState(false);
  const [checkoutLoading, setCheckoutLoading] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const agents = enabledAgents();
  const activeProject = projects.find((p) => p.id === activeProjectId) ?? null;
  const activeThread = threads.find((th) => th.id === activeThreadId) ?? null;
  const streaming = !!(activeThreadId && runningByThread[activeThreadId]);

  // Prefer thread agent; otherwise composer selection / first enabled agent
  const effectiveAgentId =
    activeThread?.agent_id
    ?? composerAgentId
    ?? agents[0]?.id
    ?? null;
  const agentMeta = agents.find((a) => a.id === effectiveAgentId);

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

  // Load git branch info for active project
  useEffect(() => {
    if (!activeProjectId) {
      setGitInfo(null);
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

  const bubbleItems: BubbleItemType[] = useMemo(() => {
    return messages.map((m) => {
      const isLast = m === messages[messages.length - 1];
      const hasContent = !!(m.content && m.content.trim().length > 0);
      // Never keep the Bubble loading spinner once content has arrived — even if
      // status is still "streaming" due to a race with loadMessages.
      const loading =
        m.role === 'assistant'
        && !hasContent
        && (m.status === 'streaming' || (streaming && isLast));
      return {
        key: m.id,
        role: m.role === 'user' ? 'user' : 'ai',
        content: m.content ?? '',
        loading,
      };
    });
  }, [messages, streaming]);

  const renderMessageFooter = useCallback(
    (msgId: string, content: string, role: 'user' | 'assistant') => {
      const msg = messages.find((m) => m.id === msgId);
      if (!msg) return null;
      const isStreamingMsg =
        msg.status === 'streaming'
        || (streaming && msg.id === messages[messages.length - 1]?.id && msg.role === 'assistant');
      if (isStreamingMsg) return null;

      const plainCopy = content.replace(/<tool-call\b[^>]*\/?>/gi, '').trim() || content;
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
          <Text type="secondary" style={{ fontSize: 11 }}>
            {formatAcpTime(msg.created_at)}
          </Text>
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
      variant: 'filled' as const,
      shape: 'corner' as const,
      avatar: userAvatar,
      header: () => (
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <Text style={{ fontSize: 13 }}>{profile.name || t('chat.you')}</Text>
        </div>
      ),
      contentRender: (content: string) => (
        <div className="aqbot-chat-text" style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
          {content}
        </div>
      ),
      footer: (content: string, info: { key?: string | number }) =>
        renderMessageFooter(String(info.key ?? ''), String(content ?? ''), 'user'),
    },
    ai: {
      placement: 'start' as const,
      variant: 'borderless' as const,
      shape: 'corner' as const,
      avatar: agentAvatar,
      header: () => {
        const name = agentMeta?.name || activeThread?.agent_id || 'Agent';
        return (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <Text style={{ fontSize: 13 }}>{name}</Text>
            <Tag color="blue" style={{ fontSize: 10, lineHeight: '16px', padding: '0 4px', margin: 0 }}>
              ACP
            </Tag>
          </div>
        );
      },
      contentRender: (content: string, { key }: { key?: string | number }) => {
        const msg = messages.find((m) => m.id === String(key));
        const isStreamingMsg =
          msg?.status === 'streaming'
          || (streaming && msg?.id === messages[messages.length - 1]?.id && msg?.role === 'assistant');
        const body = normalizeThinkTagsForMarkdown(content || '');

        // Permissions attached to this assistant message (or active stream fallback)
        const msgPermissions = msg
          ? Object.values(pendingPermissions).filter((pr) => {
              if (pr.threadId !== activeThreadId) return false;
              if (pr.messageId && pr.messageId === msg.id) return true;
              // Fallback while messageId race: show on streaming assistant
              return !pr.messageId && isStreamingMsg;
            })
          : [];

        // Empty bubble still streaming, no permission yet → status + dots
        if (!body && isStreamingMsg && msgPermissions.length === 0) {
          return (
            <div>
              <Text type="secondary" style={{ fontSize: 13 }}>
                {statusByThread[activeThreadId ?? ''] || t('agentPage.streaming', '生成中…')}
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

            {/* Permission cards sit under the message content (same as ChatView agent mode) */}
            {msgPermissions.map((pr) => (
              <PermissionCard
                key={pr.requestId}
                conversationId={pr.threadId}
                toolUseId={pr.requestId}
                toolName={pr.toolName}
                input={pr.input}
                status={
                  pr.status === 'pending'
                    ? 'pending'
                    : pr.status === 'denied'
                      ? 'denied'
                      : 'approved'
                }
                options={pr.options}
                onApprove={async (decision) => {
                  await respondPermission(pr.requestId, decision);
                }}
              />
            ))}

            {isStreamingMsg ? <StreamingDots color={token.colorTextSecondary} /> : null}
          </div>
        );
      },
      footer: (content: string, info: { key?: string | number }) =>
        renderMessageFooter(String(info.key ?? ''), String(content ?? ''), 'assistant'),
    },
  }), [
    userAvatar,
    agentAvatar,
    messages,
    profile.name,
    t,
    agentMeta?.name,
    activeThread?.agent_id,
    isDarkMode,
    darkTheme,
    lightTheme,
    themes,
    settings.code_font_family,
    streaming,
    statusByThread,
    activeThreadId,
    token.colorTextSecondary,
    pendingPermissions,
    respondPermission,
    renderMessageFooter,
  ]);

  const permissionModeItems = useMemo<MenuProps['items']>(() => [
    { key: 'default', label: t('common.permissionDefault'), icon: <Shield size={14} /> },
    {
      key: 'accept_edits',
      label: t('common.permissionAcceptEdits'),
      icon: <ShieldCheck size={14} style={{ color: '#1890ff' }} />,
    },
    {
      key: 'auto_approve',
      label: t('common.permissionAutoApprove'),
      icon: <ShieldCheck size={14} style={{ color: '#52c41a' }} />,
    },
    {
      key: 'full_access',
      label: t('common.permissionFullAccess'),
      icon: <ShieldAlert size={14} style={{ color: '#ff4d4f' }} />,
    },
  ], [t]);

  const permissionModeIcon = useMemo(() => {
    switch (permissionMode) {
      case 'accept_edits': return <ShieldCheck size={14} style={{ color: '#1890ff' }} />;
      case 'auto_approve': return <ShieldCheck size={14} style={{ color: '#52c41a' }} />;
      case 'full_access': return <ShieldAlert size={14} style={{ color: '#ff4d4f' }} />;
      default: return <Shield size={14} />;
    }
  }, [permissionMode]);

  const permissionModeLabel = useMemo(() => {
    switch (permissionMode) {
      case 'accept_edits': return t('common.permissionAcceptEdits');
      case 'auto_approve': return t('common.permissionAutoApprove');
      case 'full_access': return t('common.permissionFullAccess');
      default: return t('common.permissionDefault');
    }
  }, [permissionMode, t]);

  const handlePermissionModeChange = useCallback(async (mode: string) => {
    const apply = async () => {
      try {
        await setPermissionMode(mode);
      } catch (e) {
        console.warn(e);
      }
    };
    if (mode === 'accept_edits' || mode === 'auto_approve' || mode === 'full_access') {
      const isFull = mode === 'full_access';
      const isAuto = mode === 'auto_approve';
      modal.confirm({
        title: isFull
          ? t('agent.permissionFullAccessWarningTitle', '⚠️ 完全访问模式')
          : isAuto
            ? t('agent.permissionAutoApproveWarningTitle', '⚠️ 自动审批模式')
            : t('agent.permissionAcceptEditsWarningTitle', '⚠️ 允许编辑模式'),
        content: isFull
          ? t('agent.permissionFullAccessWarning', 'Agent 将拥有完全访问权限，可以执行任何文件操作且不受路径限制。请确保你信任当前使用的模型和 System Prompt。')
          : isAuto
            ? t('agent.permissionAutoApproveWarning', 'Agent 将自动批准所有工具权限请求，无需逐一确认。请确保你信任当前 Agent。')
            : t('agent.permissionAcceptEditsWarning', 'Agent 将自动批准文件编辑操作，无需逐一确认。请确保你了解潜在的安全风险。'),
        okText: t('common.confirm', '确认'),
        cancelText: t('common.cancel', '取消'),
        okButtonProps: isFull ? { danger: true } : undefined,
        onOk: apply,
      });
    } else {
      await apply();
    }
  }, [modal, setPermissionMode, t]);

  const agentMenuItems = useMemo<MenuProps['items']>(
    () =>
      agents.map((a) => ({
        key: a.id,
        icon: <AcpAgentIcon agentId={a.id} agentName={a.name} icon={a.icon} size={16} />,
        label: a.name,
        disabled: !!activeThreadId, // thread is bound to one agent for life
      })),
    [agents, activeThreadId],
  );

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
      setCheckoutLoading(true);
      try {
        const info = await invoke<AcpGitInfo>('acp_git_checkout', {
          projectId: activeProjectId,
          branch,
        });
        setGitInfo(info);
        messageApi.success(t('agentPage.branchSwitched', { branch, defaultValue: `已切换到 ${branch}` }));
      } catch (e) {
        messageApi.error(String(e));
      } finally {
        setCheckoutLoading(false);
      }
    },
    [activeProjectId, gitInfo?.branch, messageApi, t],
  );

  const handleSend = async (textOverride?: string) => {
    const text = (textOverride ?? value).trim();
    if (!text || sending || streaming) return;
    if (!activeProjectId) {
      messageApi.warning(t('agentPage.selectProjectFirst', '请先选择一个项目'));
      return;
    }
    if (!effectiveAgentId) {
      messageApi.warning(t('agentPage.noAgents'));
      return;
    }

    setSending(true);
    if (!textOverride) {
      setValue('');
      if (textareaRef.current) textareaRef.current.style.height = 'auto';
    }
    try {
      let threadId = activeThreadId;
      if (!threadId) {
        // First message in project → create thread then send
        const thread = await createThread(activeProjectId, effectiveAgentId, text.slice(0, 48));
        threadId = thread.id;
      }
      await sendPrompt(threadId, text);
    } catch (e) {
      messageApi.error(String(e));
    } finally {
      setSending(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void handleSend();
    }
  };

  const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setValue(e.target.value);
    const el = e.target;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  };

  const canSend =
    !sending
    && !streaming
    && value.trim().length > 0
    && !!activeProjectId
    && !!effectiveAgentId;

  // Codex-style project welcome prompts
  const projectPromptItems: PromptsItemType[] = useMemo(
    () => [
      {
        key: 'explore',
        icon: <Telescope size={16} style={{ color: '#3b82f6' }} />,
        label: t('agentPage.promptExplore', '探索并理解代码'),
      },
      {
        key: 'build',
        icon: <Hammer size={16} style={{ color: '#a855f7' }} />,
        label: t('agentPage.promptBuild', '构建新功能、应用或工具'),
      },
      {
        key: 'review',
        icon: <RefreshCw size={16} style={{ color: '#22c55e' }} />,
        label: t('agentPage.promptReview', '审查代码并提出修改建议'),
      },
      {
        key: 'fix',
        icon: <Bug size={16} style={{ color: '#f97316' }} />,
        label: t('agentPage.promptFix', '修复问题和失败'),
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

  const showProjectEmpty = !!activeProject && (!activeThread || messages.length === 0);

  const renderComposer = () => (
    <div className="px-4 pb-3 pt-1 shrink-0">
      <div
        style={{
          border: '1px solid var(--border-color)',
          borderRadius: 16,
          backgroundColor: token.colorBgContainer,
          overflow: 'hidden',
        }}
      >
        <textarea
          className="aqbot-input-textarea"
          ref={textareaRef}
          value={value}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          placeholder={t('agentPage.inputPlaceholder', 'Do anything')}
          rows={1}
          disabled={sending || !activeProjectId}
          style={{
            width: '100%',
            border: 'none',
            outline: 'none',
            resize: 'none',
            padding: '12px 16px 8px',
            fontSize: token.fontSize,
            lineHeight: 1.6,
            backgroundColor: 'transparent',
            color: token.colorText,
            fontFamily: 'inherit',
            minHeight: 44,
            maxHeight: 200,
            overflowY: 'auto',
          }}
        />
        {/* Inside input: agent (only pickable before first message) + send */}
        <div className="flex flex-wrap items-center justify-between gap-1 px-2 pb-2">
          <div className="flex flex-wrap items-center gap-0.5">
            <Dropdown
              menu={{
                items: agentMenuItems,
                selectedKeys: effectiveAgentId ? [effectiveAgentId] : [],
                onClick: ({ key }) => {
                  if (!activeThreadId) setComposerAgentId(key);
                },
              }}
              trigger={['click']}
              disabled={!!activeThreadId || agents.length === 0}
            >
              <Button
                type="text"
                size="small"
                style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12 }}
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
          </div>
          <div className="flex items-center gap-2 ml-auto">
            {streaming ? (
              <Button shape="circle" size="small" danger icon={<Square size={14} />} disabled />
            ) : (
              <Button
                type="primary"
                shape="circle"
                size="small"
                icon={<ArrowUp size={14} />}
                onClick={() => void handleSend()}
                disabled={!canSend}
                loading={sending}
              />
            )}
          </div>
        </div>
      </div>

      {/* Bottom bar: permission left · git branch right */}
      <div className="flex flex-wrap items-center justify-between gap-y-1 px-1 pt-1">
        <div className="flex flex-wrap items-center gap-1">
          <Dropdown
            menu={{
              items: permissionModeItems,
              selectedKeys: [permissionMode],
              onClick: ({ key }) => void handlePermissionModeChange(key),
            }}
            trigger={['click']}
            placement="topLeft"
          >
            <Button
              type="text"
              size="small"
              icon={permissionModeIcon}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 4,
                fontSize: 12,
                ...(permissionMode === 'full_access' ? { color: '#ff4d4f' } : {}),
              }}
            >
              {permissionModeLabel}
            </Button>
          </Dropdown>
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
                  {gitInfo.branch || t('agentPage.detachedHead', 'detached')}
                </span>
              </Button>
            </Dropdown>
          ) : activeProjectId ? (
            <Tooltip title={t('agentPage.notAGitRepo', '当前项目不是 Git 仓库')}>
              <Button
                type="text"
                size="small"
                disabled
                icon={<GitBranch size={14} />}
                style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 12 }}
              >
                {t('agentPage.noGit', '无 Git')}
              </Button>
            </Tooltip>
          ) : null}
        </div>
      </div>
    </div>
  );

  // No project selected
  if (!activeProject) {
    return (
      <div
        className="flex-1 flex items-center justify-center h-full"
        style={{ backgroundColor: token.colorBgElevated }}
      >
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={t('agentPage.selectProjectFirst', '选择左侧项目开始')}
        />
      </div>
    );
  }

  return (
    <div
      className="flex flex-col h-full min-w-0"
      style={{ backgroundColor: token.colorBgElevated, overflow: 'hidden' }}
    >
      {/* Message / welcome area */}
      <div style={{ flex: 1, minHeight: 0, position: 'relative' }}>
        {showProjectEmpty ? (
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
              <div style={{ opacity: 0.35, marginBottom: 4 }}>
                {effectiveAgentId ? (
                  <AcpAgentIcon
                    agentId={effectiveAgentId}
                    agentName={agentMeta?.name}
                    icon={agentMeta?.icon}
                    size={48}
                  />
                ) : (
                  <Bot size={48} strokeWidth={1.25} />
                )}
              </div>
              <Title
                level={3}
                style={{
                  margin: 0,
                  fontWeight: 500,
                  textAlign: 'center',
                  fontSize: 22,
                  width: '100%',
                }}
              >
                {t('agentPage.projectWelcome', {
                  name: activeProject.name,
                  defaultValue: `要在 ${activeProject.name} 内开发什么？`,
                })}
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
            `}</style>
            <Bubble.List
              className="aqbot-acp-bubble-list"
              items={bubbleItems}
              autoScroll
              role={roles as never}
              style={{
                height: '100%',
                padding: '16px 24px',
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
