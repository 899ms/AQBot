import type { TFunction } from 'i18next';
import type { Message } from '@/types';
import type { useProviderStore } from '@/stores';
import { invoke } from '@/lib/invoke';
import { copyTranscript, exportAsJSON, exportAsMarkdown, exportAsText, exportMessagesAsPNG } from '@/lib/exportChat';
import { buildExportOptions } from '@/lib/exportChatPresentation';
import { App, type theme } from 'antd';
import { Copy, FileCode, FileImage, FileText, FileType, ListChecks } from 'lucide-react';
import React, { useCallback, useMemo, useState } from 'react';

type ProviderList = ReturnType<typeof useProviderStore.getState>['providers'];
type ThemeToken = ReturnType<typeof theme.useToken>['token'];
type MessageApi = ReturnType<typeof App.useApp>['message'];

interface ChatShareExportOptions {
  activeConversation?: {
    model_id?: string | null;
    provider_id?: string | null;
    title: string;
  };
  activeConversationId: string | null;
  hasNewerMessages: boolean;
  hasOlderMessages: boolean;
  messageApi: MessageApi;
  messages: Message[];
  profileName: string;
  providers: ProviderList;
  t: TFunction;
  token: ThemeToken;
}

export function useChatShareExport({
  activeConversation,
  activeConversationId,
  hasNewerMessages,
  hasOlderMessages,
  messageApi,
  messages,
  profileName,
  providers,
  t,
  token,
}: ChatShareExportOptions) {
  const [shareSelectMode, setShareSelectMode] = useState(false);
  const [selectedShareMessageIds, setSelectedShareMessageIds] = useState<string[]>([]);
  const [shareExporting, setShareExporting] = useState(false);

  const resetShareSelection = useCallback(() => {
    setShareSelectMode(false);
    setSelectedShareMessageIds([]);
    setShareExporting(false);
  }, []);

  // ── Transcript loading and export actions ─────────────────────────────── ────────────────────────────────────────────────────
  const loadCompleteTranscript = useCallback(async () => {
    if (!activeConversationId || (!hasOlderMessages && !hasNewerMessages)) {
      return messages;
    }

    const persisted = await invoke<Message[]>('list_messages', {
      conversationId: activeConversationId,
    });
    const currentById = new Map(messages.map((item) => [item.id, item]));
    const complete = persisted.map((item) => currentById.get(item.id) ?? item);
    const persistedIds = new Set(persisted.map((item) => item.id));
    complete.push(...messages.filter((item) => !persistedIds.has(item.id)));
    return complete;
  }, [activeConversationId, hasNewerMessages, hasOlderMessages, messages]);

  const shareableMessages = useMemo(
    () => messages.filter((m) => m.role === 'user' || m.role === 'assistant'),
    [messages],
  );

  const exitShareSelectMode = useCallback(() => {
    setShareSelectMode(false);
    setSelectedShareMessageIds([]);
  }, []);

  const enterShareSelectMode = useCallback(() => {
    setShareSelectMode(true);
    setSelectedShareMessageIds([]);
  }, []);

  const toggleShareMessage = useCallback((messageId: string) => {
    setSelectedShareMessageIds((prev) => (
      prev.includes(messageId)
        ? prev.filter((id) => id !== messageId)
        : [...prev, messageId]
    ));
  }, []);

  /** Click message body/header (not interactive controls) to toggle share selection. */
  const handleShareSelectableClick = useCallback((messageId: string | undefined, e: React.MouseEvent) => {
    if (!shareSelectMode || !messageId) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest(
      'a, button, input, textarea, .ant-checkbox-wrapper, .ant-checkbox, [data-share-ignore="true"]',
    )) {
      return;
    }
    toggleShareMessage(messageId);
  }, [shareSelectMode, toggleShareMessage]);

  const wrapShareSelectableContent = useCallback((messageId: string | undefined, node: React.ReactNode) => {
    if (!shareSelectMode || !messageId) return node;
    return (
      <div
        data-share-selectable="true"
        onClick={(e) => handleShareSelectableClick(messageId, e)}
        style={{ cursor: 'pointer' }}
      >
        {node}
      </div>
    );
  }, [handleShareSelectableClick, shareSelectMode]);

  const getShareSelectBubbleStyles = useCallback((messageId: string | undefined) => {
    if (!shareSelectMode || !messageId) return undefined;
    const selected = selectedShareMessageIds.includes(messageId);
    return {
      root: { cursor: 'pointer' as const },
      content: {
        cursor: 'pointer' as const,
        boxShadow: selected ? `0 0 0 2px ${token.colorPrimary}` : undefined,
        transition: 'box-shadow 0.15s ease',
      },
    };
  }, [selectedShareMessageIds, shareSelectMode, token.colorPrimary]);

  const selectAllShareMessages = useCallback(() => {
    setSelectedShareMessageIds(shareableMessages.map((m) => m.id));
  }, [shareableMessages]);

  const getSelectedShareMessagesOrdered = useCallback(() => {
    const selected = new Set(selectedShareMessageIds);
    return shareableMessages.filter((m) => selected.has(m.id));
  }, [selectedShareMessageIds, shareableMessages]);

  const buildChatExportOptions = useCallback((includeThinking = false) => ({
    ...buildExportOptions({
      userName: profileName,
      theme: {
        colorPrimary: token.colorPrimary,
        colorPrimaryBg: token.colorPrimaryBg,
        colorPrimaryBorder: token.colorPrimaryBorder,
        colorFillSecondary: token.colorFillSecondary,
      },
      providers,
      conversationModelId: activeConversation?.model_id,
      conversationProviderId: activeConversation?.provider_id,
    }),
    includeThinking,
  }), [
    activeConversation?.model_id,
    activeConversation?.provider_id,
    profileName,
    providers,
    token.colorFillSecondary,
    token.colorPrimary,
    token.colorPrimaryBg,
    token.colorPrimaryBorder,
  ]);

  const exportSelectedShare = useCallback(async (format: 'png' | 'md' | 'copy-md') => {
    const selected = getSelectedShareMessagesOrdered();
    if (selected.length === 0) {
      messageApi.warning(t('chat.shareSelectNone'));
      return;
    }
    const title = activeConversation?.title ?? 'chat';
    const exportOptions = buildChatExportOptions(false);
    setShareExporting(true);
    try {
      if (format === 'png') {
        const ok = await exportMessagesAsPNG(selected, title, exportOptions);
        if (ok) {
          messageApi.success(t('chat.exportSuccess'));
          exitShareSelectMode();
        }
      } else if (format === 'md') {
        const ok = await exportAsMarkdown(selected, title, exportOptions);
        if (ok) {
          messageApi.success(t('chat.exportSuccess'));
          exitShareSelectMode();
        }
      } else {
        const ok = await copyTranscript(selected, title, 'markdown', exportOptions);
        if (ok) {
          messageApi.success(t('chat.copied'));
          exitShareSelectMode();
        }
      }
    } catch (e) {
      console.error('Share selected messages failed:', e);
      messageApi.error(t('chat.exportFailed'));
    } finally {
      setShareExporting(false);
    }
  }, [activeConversation?.title, buildChatExportOptions, exitShareSelectMode, getSelectedShareMessagesOrdered, messageApi, t]);

  const exportMenuItems = useMemo(
    () => [
      {
        key: 'select-share',
        label: t('chat.shareSelectMessages'),
        icon: <ListChecks size={14} />,
        onClick: () => {
          if (shareableMessages.length === 0) {
            messageApi.warning(t('chat.noMessages'));
            return;
          }
          enterShareSelectMode();
        },
      },
      {
        key: 'copy-md',
        label: t('chat.copyMarkdown'),
        icon: <Copy size={14} />,
        onClick: async () => {
          try {
            const transcript = await loadCompleteTranscript();
            if (transcript.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await copyTranscript(
              transcript,
              activeConversation?.title ?? 'chat',
              'markdown',
              buildChatExportOptions(false),
            );
            if (ok) messageApi.success(t('chat.copied'));
          } catch (e) { console.error('Copy MD failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
      {
        key: 'png',
        label: t('chat.exportPng'),
        icon: <FileImage size={14} />,
        onClick: async () => {
          try {
            // Data-driven PNG avoids viewport clipping and action-icon layout bugs.
            const transcript = await loadCompleteTranscript();
            const shareable = transcript.filter((m) => m.role === 'user' || m.role === 'assistant');
            if (shareable.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await exportMessagesAsPNG(
              shareable,
              activeConversation?.title ?? 'chat',
              buildChatExportOptions(false),
            );
            if (ok) messageApi.success(t('chat.exportSuccess'));
          } catch (e) { console.error('Export PNG failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
      {
        key: 'md',
        label: t('chat.exportMd'),
        icon: <FileCode size={14} />,
        onClick: async () => {
          try {
            const transcript = await loadCompleteTranscript();
            if (transcript.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await exportAsMarkdown(
              transcript,
              activeConversation?.title ?? 'chat',
              buildChatExportOptions(true),
            );
            if (ok) messageApi.success(t('chat.exportSuccess'));
          } catch (e) { console.error('Export MD failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
      {
        key: 'export-md-no-thinking',
        label: t('chat.exportMdNoThinking'),
        icon: <FileCode size={14} />,
        onClick: async () => {
          try {
            const transcript = await loadCompleteTranscript();
            if (transcript.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await exportAsMarkdown(
              transcript,
              activeConversation?.title ?? 'chat',
              buildChatExportOptions(false),
            );
            if (ok) messageApi.success(t('chat.exportSuccess'));
          } catch (e) { console.error('Export MD (no thinking) failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
      {
        key: 'txt',
        label: t('chat.exportTxt'),
        icon: <FileType size={14} />,
        onClick: async () => {
          try {
            const transcript = await loadCompleteTranscript();
            if (transcript.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await exportAsText(
              transcript,
              activeConversation?.title ?? 'chat',
              buildChatExportOptions(true),
            );
            if (ok) messageApi.success(t('chat.exportSuccess'));
          } catch (e) { console.error('Export TXT failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
      {
        key: 'export-txt-no-thinking',
        label: t('chat.exportTxtNoThinking'),
        icon: <FileType size={14} />,
        onClick: async () => {
          try {
            const transcript = await loadCompleteTranscript();
            if (transcript.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await exportAsText(
              transcript,
              activeConversation?.title ?? 'chat',
              buildChatExportOptions(false),
            );
            if (ok) messageApi.success(t('chat.exportSuccess'));
          } catch (e) { console.error('Export TXT (no thinking) failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
      {
        key: 'json',
        label: t('chat.exportJson'),
        icon: <FileText size={14} />,
        onClick: async () => {
          try {
            const transcript = await loadCompleteTranscript();
            if (transcript.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await exportAsJSON(
              transcript,
              activeConversation?.title ?? 'chat',
              buildChatExportOptions(true),
            );
            if (ok) messageApi.success(t('chat.exportSuccess'));
          } catch (e) { console.error('Export JSON failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
      {
        key: 'export-json-no-thinking',
        label: t('chat.exportJsonNoThinking'),
        icon: <FileText size={14} />,
        onClick: async () => {
          try {
            const transcript = await loadCompleteTranscript();
            if (transcript.length === 0) { messageApi.warning(t('chat.noMessages')); return; }
            const ok = await exportAsJSON(
              transcript,
              activeConversation?.title ?? 'chat',
              buildChatExportOptions(false),
            );
            if (ok) messageApi.success(t('chat.exportSuccess'));
          } catch (e) { console.error('Export JSON (no thinking) failed:', e); messageApi.error(t('chat.exportFailed')); }
        },
      },
    ],
    [activeConversation, buildChatExportOptions, enterShareSelectMode, loadCompleteTranscript, messageApi, shareableMessages.length, t],
  );


  return {
    exitShareSelectMode,
    exportMenuItems,
    exportSelectedShare,
    getShareSelectBubbleStyles,
    handleShareSelectableClick,
    resetShareSelection,
    selectAllShareMessages,
    selectedShareMessageIds,
    shareExporting,
    shareSelectMode,
    toggleShareMessage,
    wrapShareSelectableContent,
  };
}
