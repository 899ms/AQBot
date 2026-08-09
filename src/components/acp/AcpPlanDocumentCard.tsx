import { useMemo, useRef, useState } from 'react';
import { Button, ConfigProvider, Modal, Typography, theme } from 'antd';
import {
  Check,
  Copy,
  ListTodo,
  Maximize2,
  Minimize2,
  MessageSquarePlus,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores';
import { useResolvedDarkMode } from '@/hooks/useResolvedDarkMode';
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard';
import {
  ChatMarkdownRenderer,
  getChatCodeThemes,
} from '@/components/chat/chatMarkdownShared';
import { ChatMessageRenderBoundary } from '@/components/chat/ChatMessageRenderBoundary';
import type { AcpPlanDocument } from '@/stores/acpStore';

const { Text } = Typography;

/** Optional host callback so inline plan nodes can "bring into context". */
type AcpPlanContextHandler = (content: string) => void;
let planContextHandler: AcpPlanContextHandler | null = null;

export function setAcpPlanContextHandler(handler: AcpPlanContextHandler | null) {
  planContextHandler = handler;
}

export function requestAcpPlanAddToContext(content: string) {
  planContextHandler?.(content);
}

/** Collapsed plan body height in the timeline / composer. */
export const PLAN_DOCUMENT_MAX_HEIGHT = 280;
/** Expanded plan viewer — fill almost the whole app shell height. */
export const PLAN_DOCUMENT_EXPANDED_MAX_HEIGHT = 'min(92vh, calc(100dvh - 56px))';

export function extractAcpPlanContent(
  input: Record<string, unknown> | null | undefined,
  extras?: {
    description?: string | null;
    title?: string | null;
    question?: string | null;
  },
): string {
  const source = input ?? {};
  const candidates = [
    extras?.description,
    source.planContent,
    source.plan_content,
    source.plan,
    source.content,
    source.description,
    extras?.question,
    extras?.title,
    source.title,
  ];
  for (const value of candidates) {
    if (typeof value === 'string' && value.trim()) return value;
  }
  return '';
}

function outcomeLabel(
  status: AcpPlanDocument['status'],
  t: (key: string) => string,
): string | null {
  switch (status) {
    case 'approved':
      return t('agentPage.interactionPlanOutcomeApproved');
    case 'cancelled':
      return t('agentPage.interactionPlanOutcomeChanges');
    case 'abandoned':
      return t('agentPage.interactionPlanOutcomeAbandoned');
    case 'expired':
      return t('agentPage.interactionPlanOutcomeExpired');
    case 'pending':
      return t('agentPage.interactionPlanOutcomePending');
    default:
      return null;
  }
}

function outcomeColor(
  status: AcpPlanDocument['status'],
  token: ReturnType<typeof theme.useToken>['token'],
): string {
  switch (status) {
    case 'approved':
      return token.colorSuccess;
    case 'cancelled':
      return token.colorWarning;
    case 'abandoned':
    case 'expired':
      return token.colorTextSecondary;
    case 'pending':
      return token.colorPrimary;
    default:
      return token.colorTextSecondary;
  }
}

export interface AcpPlanMarkdownBodyProps {
  content: string;
  className?: string;
  maxHeight?: number | string;
  expanded?: boolean;
}

/**
 * Plan markdown body — same ChatMarkdownRenderer stack as ACP conversation
 * bubbles (`customId="acp"`, theme/code-font from settings, `aqbot-chat-markdown`).
 */
export function AcpPlanMarkdownBody({
  content,
  className,
  maxHeight,
  expanded = false,
}: AcpPlanMarkdownBodyProps) {
  const { token } = theme.useToken();
  const settings = useSettingsStore((s) => s.settings);
  const isDarkMode = useResolvedDarkMode(settings.theme_mode ?? 'system');
  const { darkTheme, lightTheme, themes } = useMemo(
    () => getChatCodeThemes(settings.code_theme, settings.code_theme_light),
    [settings.code_theme, settings.code_theme_light],
  );

  return (
    <div
      className={['aqbot-acp-plan-markdown', 'aqbot-chat-markdown', className]
        .filter(Boolean)
        .join(' ')}
      style={{
        minWidth: 0,
        minHeight: 0,
        width: '100%',
        flex: 1,
        maxHeight: expanded ? undefined : maxHeight,
        overflowY: 'auto',
        overflowX: 'hidden',
        padding: 10,
        borderRadius: token.borderRadius,
        background: token.colorFillQuaternary,
      }}
    >
      <style>{`
        .aqbot-acp-plan-markdown table {
          display: block;
          max-width: 100%;
          overflow-x: auto;
        }
        .aqbot-acp-plan-markdown .markstream-react {
          overflow: hidden;
          min-width: 0;
        }
        .aqbot-acp-plan-markdown .code-block-node,
        .aqbot-acp-plan-markdown .code-block-container {
          overflow-x: auto;
          max-width: 100%;
          min-width: 0 !important;
          width: 100%;
          box-sizing: border-box;
        }
      `}</style>
      {content.trim() ? (
        <ChatMessageRenderBoundary
          fallback={
            <div style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{content}</div>
          }
        >
          <ChatMarkdownRenderer
            content={content}
            isDark={isDarkMode}
            final
            codeBlockDarkTheme={darkTheme}
            codeBlockLightTheme={lightTheme}
            codeBlockThemes={themes}
            codeFontFamily={settings.code_font_family || undefined}
            customId="acp"
          />
        </ChatMessageRenderBoundary>
      ) : (
        <Text type="secondary">
          {/* fallback only — callers usually pass non-empty content */}
        </Text>
      )}
    </div>
  );
}

export interface AcpPlanDocumentCardProps {
  document: AcpPlanDocument;
  /** Hide outcome badge (e.g. while reviewing in composer). */
  hideOutcome?: boolean;
  /** When true, body starts expanded to near-fullscreen. */
  defaultExpanded?: boolean;
  onAddToContext?: (content: string) => void;
}

/**
 * Timeline / historical plan card: markdown body with max height, copy,
 * expand, and optional “bring into context”.
 */
export function AcpPlanDocumentCard({
  document,
  hideOutcome = false,
  defaultExpanded = false,
  onAddToContext,
}: AcpPlanDocumentCardProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { copy: copyText, isCopiedFor } = useCopyToClipboard();
  const [expanded, setExpanded] = useState(defaultExpanded);
  const expandButtonRef = useRef<HTMLButtonElement>(null);
  const restoreFocusRef = useRef(false);
  const title = document.title?.trim()
    || t('agentPage.interactionPlanReviewTitle');
  const outcome = hideOutcome ? null : outcomeLabel(
    document.status,
    (key) => t(key),
  );
  const copied = isCopiedFor(document.content);

  const closeFullscreen = () => {
    restoreFocusRef.current = true;
    setExpanded(false);
  };
  const restoreTriggerFocus = () => {
    if (!restoreFocusRef.current) return;
    restoreFocusRef.current = false;
    window.requestAnimationFrame(() => expandButtonRef.current?.focus());
  };

  const planCard = (
      <div
        style={{
          display: 'flex',
          minWidth: 0,
          width: '100%',
          height: expanded ? '100%' : undefined,
          flexDirection: 'column',
          gap: 8,
          ...(expanded
            ? {}
            : {
                padding: 10,
                borderRadius: token.borderRadiusLG,
                border: `1px solid ${token.colorBorderSecondary}`,
                background: token.colorBgContainer,
              }),
        }}
      >
        <div
          style={{
            display: 'flex',
            minWidth: 0,
            flexShrink: 0,
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 8,
          }}
        >
          <div
            style={{
              display: 'flex',
              minWidth: 0,
              flexWrap: 'wrap',
              alignItems: 'center',
              gap: 8,
            }}
          >
            {!expanded ? (
              <>
                <ListTodo size={14} style={{ color: token.colorPrimary, flexShrink: 0 }} />
                <Text strong style={{ overflowWrap: 'anywhere' }}>{title}</Text>
              </>
            ) : null}
            {outcome ? (
              <Text
                style={{
                  fontSize: 12,
                  color: outcomeColor(document.status, token),
                  flexShrink: 0,
                }}
              >
                {outcome}
              </Text>
            ) : null}
          </div>
          <div style={{ display: 'flex', flexShrink: 0, alignItems: 'center', gap: 2 }}>
            <Button
              type="text"
              size="small"
              icon={copied
                ? <Check size={14} style={{ color: token.colorSuccess }} />
                : <Copy size={14} />}
              aria-label={t('chat.copy')}
              onClick={() => {
                void copyText(document.content);
              }}
            />
            {onAddToContext ? (
              <Button
                type="text"
                size="small"
                icon={<MessageSquarePlus size={14} />}
                aria-label={t('agentPage.interactionPlanAddToContext')}
                onClick={() => onAddToContext(document.content)}
              />
            ) : null}
            <Button
              ref={expanded ? undefined : expandButtonRef}
              type="text"
              size="small"
              icon={expanded ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
              aria-label={expanded
                ? t('agentPage.interactionPlanExitFullscreen')
                : t('agentPage.interactionPlanFullscreen')}
              aria-pressed={expanded}
              onClick={() => {
                if (expanded) closeFullscreen();
                else setExpanded(true);
              }}
            />
          </div>
        </div>

        {document.content.trim() ? (
          <div style={{ minWidth: 0, minHeight: 0, flex: 1, overflow: 'hidden', display: 'flex' }}>
            <AcpPlanMarkdownBody
              content={document.content}
              maxHeight={PLAN_DOCUMENT_MAX_HEIGHT}
              expanded={expanded}
            />
          </div>
        ) : (
          <Text type="secondary">{t('agentPage.interactionPlanEmpty')}</Text>
        )}

        {document.feedback?.trim() ? (
          <Text type="secondary" style={{ fontSize: 12, overflowWrap: 'anywhere' }}>
            {t('agentPage.interactionPlanFeedbackLabel')}: {document.feedback.trim()}
          </Text>
        ) : null}
      </div>
  );

  return (
    <ConfigProvider button={{ autoInsertSpace: false }}>
      {!expanded ? planCard : null}
      <Modal
        open={expanded}
        title={title}
        footer={null}
        closable={false}
        keyboard
        mask={{ enabled: true, blur: true, closable: true }}
        onCancel={closeFullscreen}
        afterClose={restoreTriggerFocus}
        width="calc(100vw - 32px)"
        zIndex={1100}
        style={{ top: 16, maxWidth: 'calc(100vw - 32px)', paddingBottom: 0 }}
        styles={{
          wrapper: { position: 'fixed' },
          container: {
            display: 'flex',
            height: PLAN_DOCUMENT_EXPANDED_MAX_HEIGHT,
            maxHeight: PLAN_DOCUMENT_EXPANDED_MAX_HEIGHT,
            flexDirection: 'column',
            boxSizing: 'border-box',
          },
          header: { flexShrink: 0 },
          body: {
            display: 'flex',
            minHeight: 0,
            flex: 1,
            overflow: 'hidden',
            overscrollBehavior: 'contain',
          },
        }}
        focusable={{ trap: true, focusTriggerAfterClose: false }}
        transitionName=""
        maskTransitionName=""
        destroyOnHidden
      >
        {expanded ? planCard : null}
      </Modal>
    </ConfigProvider>
  );
}
