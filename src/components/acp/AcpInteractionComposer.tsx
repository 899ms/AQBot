import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { Button, ConfigProvider, Input, Typography, theme } from 'antd';
import { Maximize2, Minimize2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AcpPermissionRequest, AcpQuestionnaireSubmission } from '@/stores/acpStore';
import {
  AcpPlanMarkdownBody,
  PLAN_DOCUMENT_EXPANDED_MAX_HEIGHT,
  PLAN_DOCUMENT_MAX_HEIGHT,
  extractAcpPlanContent,
} from './AcpPlanDocumentCard';
import { AcpQuestionnaireComposer, parseAcpQuestionnaire } from './AcpQuestionnaireComposer';

const { Text } = Typography;

/** Scrollable prompt/details region; action buttons stay fixed below. */
const INTERACTION_CONTENT_MAX_HEIGHT = 260;

export type AcpInteractionKind = 'permission' | 'question' | 'plan_review';
export type AcpInteractionOption = AcpPermissionRequest['options'][number] & {
  kind?: string | null;
  description?: string | null;
};
export type AcpInteractionRequest = Omit<AcpPermissionRequest, 'options'> & {
  kind?: AcpInteractionKind;
  title?: string | null;
  description?: string | null;
  question?: string | null;
  options: AcpInteractionOption[];
};
export interface AcpInteractionComposerProps {
  request: AcpInteractionRequest;
  onSubmit: (submission: AcpInteractionSubmission) => Promise<void>;
}
export type AcpInteractionSubmission =
  | { optionId: string; feedback?: string }
  | { questionnaire: AcpQuestionnaireSubmission };
type Translate = (key: string, fallback: string) => string;

/** Synthetic option: allow this tool for the rest of the AQBot thread/session. */
export const ACP_SESSION_ALWAYS_ALLOW_OPTION_ID = '__aqbot_session_always_allow';

function normalizedToken(value: unknown): string {
  return String(value ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
}

export function isAllowAlwaysOption(option: Pick<AcpInteractionOption, 'id' | 'kind'>): boolean {
  const identity = `${normalizedToken(option.id)} ${normalizedToken(option.kind)}`;
  return identity.includes('allowalways')
    || option.id === ACP_SESSION_ALWAYS_ALLOW_OPTION_ID;
}

export function isAllowOnceOption(option: Pick<AcpInteractionOption, 'id' | 'kind'>): boolean {
  const identity = `${normalizedToken(option.id)} ${normalizedToken(option.kind)}`;
  return identity.includes('allowonce')
    || identity === 'allow'
    || identity === 'approved'
    || identity === 'approve';
}

export function findAgentAllowOption(
  options: AcpInteractionOption[],
): AcpInteractionOption | undefined {
  return options.find((option) => isAllowAlwaysOption(option)
    && option.id !== ACP_SESSION_ALWAYS_ALLOW_OPTION_ID)
    ?? options.find((option) => isAllowOnceOption(option))
    ?? options.find((option) => {
      const identity = `${normalizedToken(option.id)} ${normalizedToken(option.kind)}`;
      return identity.includes('allow') && !identity.includes('reject') && !identity.includes('deny');
    });
}

/**
 * Ensure permission prompts always expose "始终允许" (session-scoped).
 * Agents often only advertise allow_once + reject; we inject a synthetic option
 * that the store maps onto a real agent allow option and remembers for the thread.
 */
export function ensureSessionAlwaysAllowOption(
  options: AcpInteractionOption[],
  kind: AcpInteractionKind = 'permission',
): AcpInteractionOption[] {
  if (kind !== 'permission') return options;
  if (options.some((option) => isAllowAlwaysOption(option))) return options;
  if (!findAgentAllowOption(options)) return options;

  const alwaysOption: AcpInteractionOption = {
    id: ACP_SESSION_ALWAYS_ALLOW_OPTION_ID,
    label: '始终允许',
    kind: 'AllowAlways',
    variant: 'default',
  };

  const allowIndex = options.findIndex((option) => isAllowOnceOption(option));
  if (allowIndex >= 0) {
    return [
      ...options.slice(0, allowIndex + 1),
      alwaysOption,
      ...options.slice(allowIndex + 1),
    ];
  }
  return [alwaysOption, ...options];
}

function interactionTitle(kind: AcpInteractionKind, translate: Translate): string {
  if (kind === 'question') {
    return translate('agentPage.interactionQuestionTitle', '需要你的回答');
  }
  if (kind === 'plan_review') {
    return translate('agentPage.interactionPlanReviewTitle', '审核计划');
  }
  return translate('agentPage.interactionPermissionTitle', '需要权限');
}

function knownOptionLabel(
  requestKind: AcpInteractionKind,
  option: AcpInteractionOption,
  translate: Translate,
): string {
  if (requestKind === 'question') return option.label;
  const id = normalizedToken(option.id);
  const kind = normalizedToken(option.kind);
  const identity = `${id} ${kind}`;

  if (requestKind === 'plan_review') {
    if (id === 'approved') {
      return translate('agentPage.interactionPlanExecute', '立即执行');
    }
    if (id === 'cancelled') {
      return translate('agentPage.interactionPlanRequestChanges', '进行改变');
    }
    if (id === 'abandoned') {
      return translate('agentPage.interactionPlanCancel', '取消');
    }
  }

  if (identity.includes('allowalways')) {
    return translate('agentPage.interactionAllowAlways', '始终允许');
  }
  if (identity.includes('allowonce') || id === 'approved' || id === 'approve') {
    return translate('agentPage.interactionAllowOnce', '允许一次');
  }
  if (
    identity.includes('reject')
    || identity.includes('deny')
    || identity.includes('cancel')
    || id === 'abandoned'
  ) {
    return translate('agentPage.interactionDeny', '拒绝');
  }
  return option.label;
}

function promptText(request: AcpInteractionRequest, kind: AcpInteractionKind): string | null {
  const input = request.input ?? {};
  if (kind === 'question') {
    const questions = Array.isArray(input.questions) ? input.questions : [];
    const firstQuestion = questions[0];
    const nestedQuestion = firstQuestion && typeof firstQuestion === 'object'
      ? (firstQuestion as Record<string, unknown>).question
      : null;
    const value = request.question ?? input.question ?? nestedQuestion ?? request.description;
    return typeof value === 'string' && value.trim() ? value : null;
  }
  if (kind === 'plan_review') {
    const value = extractAcpPlanContent(input, {
      description: request.description,
      title: request.title,
      question: request.question,
    });
    return value.trim() ? value : null;
  }
  return typeof request.description === 'string' && request.description.trim()
    ? request.description
    : null;
}

function optionAppearance(
  requestKind: AcpInteractionKind,
  option: AcpInteractionOption,
): { primary: boolean; danger: boolean } {
  const identity = `${normalizedToken(option.id)} ${normalizedToken(option.kind)}`;
  const danger = option.variant === 'danger'
    || identity.includes('reject')
    || identity.includes('deny')
    || identity.includes('abandon');
  // "始终允许" stays secondary; only allow-once / plan approve are primary.
  const primary = !danger && (
    option.variant === 'primary'
    || identity.includes('allowonce')
    || (requestKind === 'plan_review' && normalizedToken(option.id) === 'approved')
  );
  return { primary, danger };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function findPlanOption(
  options: AcpInteractionOption[],
  id: 'approved' | 'cancelled' | 'abandoned',
): AcpInteractionOption | undefined {
  return options.find((option) => normalizedToken(option.id) === id);
}

export function AcpInteractionComposer({
  request,
  onSubmit,
}: AcpInteractionComposerProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const titleId = useId();
  const [loadingOptionId, setLoadingOptionId] = useState<string | null>(null);
  const [submissionError, setSubmissionError] = useState<string | null>(null);
  const [planFeedbackMode, setPlanFeedbackMode] = useState(false);
  const [planFeedback, setPlanFeedback] = useState('');
  const [planExpanded, setPlanExpanded] = useState(false);
  const activeRequestIdRef = useRef(request.requestId);
  const firstOptionRef = useRef<HTMLButtonElement>(null);
  const mountedRef = useRef(true);
  const questionnaire = useMemo(
    () => parseAcpQuestionnaire(request.input ?? {}),
    [request.input, request.requestId],
  );
  activeRequestIdRef.current = request.requestId;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    setLoadingOptionId(null);
    setSubmissionError(null);
    setPlanFeedbackMode(false);
    setPlanFeedback('');
    setPlanExpanded(false);
    const frame = window.requestAnimationFrame(() => firstOptionRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [request.requestId]);

  if (request.status !== 'pending') return null;

  const kind = request.kind ?? 'permission';
  if (kind === 'question' && questionnaire) {
    return (
      <AcpQuestionnaireComposer
        request={request}
        questionnaire={questionnaire}
        onSubmit={(submission) => onSubmit({ questionnaire: submission })}
      />
    );
  }
  const translate: Translate = (key, fallback) => t(key, fallback);
  const title = interactionTitle(kind, translate);
  const prompt = promptText(request, kind);
  const inputJson = JSON.stringify(request.input ?? {}, null, 2);
  const submitting = loadingOptionId !== null;
  const displayOptions = ensureSessionAlwaysAllowOption(request.options, kind);

  const submitOption = async (optionId: string, feedback?: string) => {
    const requestId = request.requestId;
    setLoadingOptionId(optionId);
    setSubmissionError(null);
    try {
      await onSubmit({ optionId, ...(feedback?.trim() ? { feedback: feedback.trim() } : {}) });
    } catch (error) {
      if (mountedRef.current && activeRequestIdRef.current === requestId) {
        setSubmissionError(errorMessage(error));
      }
    } finally {
      if (mountedRef.current && activeRequestIdRef.current === requestId) {
        setLoadingOptionId(null);
      }
    }
  };

  // ── Plan review: content in composer with max height + 3 action buttons ──
  if (kind === 'plan_review') {
    const approveOption = findPlanOption(request.options, 'approved');
    const changeOption = findPlanOption(request.options, 'cancelled');
    const cancelOption = findPlanOption(request.options, 'abandoned');
    const planBody = prompt ?? '';

    const submitPlanFeedback = () => {
      if (!changeOption) return;
      const text = planFeedback.trim();
      if (!text) {
        setSubmissionError(t('agentPage.interactionPlanFeedbackRequired', '请输入修改意见'));
        return;
      }
      void submitOption(changeOption.id, text);
    };

    return (
      <ConfigProvider button={{ autoInsertSpace: false }}>
        {planExpanded ? (
          <div
            role="presentation"
            onClick={() => setPlanExpanded(false)}
            style={{
              position: 'fixed',
              inset: 0,
              zIndex: 1099,
              background: 'rgba(0, 0, 0, 0.45)',
            }}
          />
        ) : null}
        <form
          aria-labelledby={titleId}
          aria-busy={submitting}
          onSubmit={(event) => {
            event.preventDefault();
            if (planFeedbackMode) submitPlanFeedback();
          }}
          style={{
            display: 'flex',
            minWidth: 0,
            width: planExpanded ? 'auto' : '100%',
            maxHeight: planExpanded ? undefined : 'min(55vh, 480px)',
            flexDirection: 'column',
            gap: 10,
            touchAction: 'manipulation',
            ...(planExpanded
              ? {
                  position: 'fixed',
                  inset: 16,
                  zIndex: 1100,
                  padding: 16,
                  borderRadius: 16,
                  background: token.colorBgElevated,
                  border: `1px solid ${token.colorBorderSecondary}`,
                  boxShadow: token.boxShadowSecondary,
                  maxHeight: PLAN_DOCUMENT_EXPANDED_MAX_HEIGHT,
                  height: PLAN_DOCUMENT_EXPANDED_MAX_HEIGHT,
                  boxSizing: 'border-box' as const,
                }
              : {}),
          }}
        >
          <style>{`
            .aqbot-acp-interaction-option:focus-visible {
              box-shadow: 0 0 0 3px ${token.colorPrimaryBorder};
              border-radius: ${token.borderRadius}px;
            }
          `}</style>

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
              <Text id={titleId} strong style={{ overflowWrap: 'anywhere' }}>
                {title}
              </Text>
              {request.toolName ? (
                <code
                  translate="no"
                  style={{
                    minWidth: 0,
                    maxWidth: '100%',
                    padding: '1px 4px',
                    borderRadius: token.borderRadiusSM,
                    background: token.colorFillQuaternary,
                    overflowWrap: 'anywhere',
                  }}
                >
                  {request.toolName}
                </code>
              ) : null}
            </div>
            <Button
              type="text"
              size="small"
              icon={planExpanded ? <Minimize2 size={16} /> : <Maximize2 size={16} />}
              aria-label={planExpanded
                ? t('agentPage.interactionPlanExitFullscreen', '退出全屏')
                : t('agentPage.interactionPlanFullscreen', '全屏查看')}
              aria-pressed={planExpanded}
              onClick={() => setPlanExpanded((value) => !value)}
            />
          </div>

          {/* Scrollable plan body — same markdown stack as conversation bubbles */}
          {planBody ? (
            <div style={{ minWidth: 0, minHeight: 0, flex: 1, overflow: 'hidden', display: 'flex' }}>
              <AcpPlanMarkdownBody
                content={planBody}
                maxHeight={PLAN_DOCUMENT_MAX_HEIGHT}
                expanded={planExpanded}
              />
            </div>
          ) : (
            <Text type="secondary">{t('agentPage.interactionPlanEmpty', '暂无计划内容')}</Text>
          )}

          {/* Fixed actions: never scrolled away */}
          {planFeedbackMode ? (
            <div style={{ display: 'flex', flexShrink: 0, flexDirection: 'column', gap: 8 }}>
              <Input.TextArea
                autoFocus
                value={planFeedback}
                disabled={submitting}
                rows={3}
                placeholder={t('agentPage.interactionPlanFeedbackPlaceholder', '描述希望如何调整计划…')}
                aria-label={t('agentPage.interactionPlanFeedbackPlaceholder', '描述希望如何调整计划…')}
                onChange={(event) => {
                  setPlanFeedback(event.target.value);
                  if (submissionError) setSubmissionError(null);
                }}
              />
              <div style={{ display: 'flex', flexWrap: 'wrap', justifyContent: 'flex-end', gap: 8 }}>
                <Button
                  disabled={submitting}
                  onClick={() => {
                    setPlanFeedbackMode(false);
                    setPlanFeedback('');
                    setSubmissionError(null);
                  }}
                >
                  {t('common.back', '返回')}
                </Button>
                <Button
                  type="primary"
                  disabled={submitting}
                  loading={loadingOptionId === changeOption?.id}
                  onClick={submitPlanFeedback}
                >
                  {t('agentPage.interactionPlanSubmitFeedback', '提交修改意见')}
                </Button>
              </div>
            </div>
          ) : (
            <div
              style={{
                display: 'grid',
                flexShrink: 0,
                gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
                gap: 8,
              }}
            >
              <Button
                ref={firstOptionRef}
                className="aqbot-acp-interaction-option"
                type="primary"
                disabled={submitting || !approveOption}
                loading={loadingOptionId === approveOption?.id}
                onClick={() => approveOption && void submitOption(approveOption.id)}
                style={{ height: 'auto', paddingBlock: 8, whiteSpace: 'normal' }}
              >
                {t('agentPage.interactionPlanExecute', '立即执行')}
              </Button>
              <Button
                className="aqbot-acp-interaction-option"
                disabled={submitting || !changeOption}
                onClick={() => {
                  setPlanFeedbackMode(true);
                  setSubmissionError(null);
                }}
                style={{ height: 'auto', paddingBlock: 8, whiteSpace: 'normal' }}
              >
                {t('agentPage.interactionPlanRequestChanges', '进行改变')}
              </Button>
              <Button
                className="aqbot-acp-interaction-option"
                danger
                disabled={submitting || !cancelOption}
                loading={loadingOptionId === cancelOption?.id}
                onClick={() => cancelOption && void submitOption(cancelOption.id)}
                aria-label={t('agentPage.interactionPlanCancel', '取消')}
                style={{ height: 'auto', paddingBlock: 8, whiteSpace: 'normal' }}
              >
                {t('agentPage.interactionPlanCancel', '取消')}
              </Button>
            </div>
          )}

          {submissionError ? (
            <Text type="danger" role="alert" style={{ flexShrink: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
              {t('agentPage.interactionSubmitFailed', '提交失败，请重试')}: {submissionError}
            </Text>
          ) : null}
        </form>
      </ConfigProvider>
    );
  }

  // ── Permission / generic interaction ──
  return (
    <ConfigProvider button={{ autoInsertSpace: false }}>
    <form
      aria-labelledby={titleId}
      aria-busy={submitting}
      onSubmit={(event) => event.preventDefault()}
      style={{
        display: 'flex',
        minWidth: 0,
        width: '100%',
        maxHeight: 'min(50vh, 440px)',
        flexDirection: 'column',
        gap: 10,
        touchAction: 'manipulation',
      }}
    >
      <style>{`
        .aqbot-acp-interaction-option:focus-visible,
        .aqbot-acp-interaction-summary:focus-visible {
          box-shadow: 0 0 0 3px ${token.colorPrimaryBorder};
          border-radius: ${token.borderRadius}px;
        }
      `}</style>
      <div
        role="group"
        aria-labelledby={titleId}
        aria-live="polite"
        style={{
          display: 'flex',
          minWidth: 0,
          minHeight: 0,
          flex: 1,
          flexDirection: 'column',
          gap: 10,
        }}
      >
        <div style={{ display: 'flex', minWidth: 0, flexShrink: 0, flexWrap: 'wrap', gap: 8 }}>
          <Text id={titleId} strong style={{ overflowWrap: 'anywhere' }}>
            {title}
          </Text>
          {request.toolName ? (
            <code
              translate="no"
              style={{
                minWidth: 0,
                maxWidth: '100%',
                padding: '1px 4px',
                borderRadius: token.borderRadiusSM,
                background: token.colorFillQuaternary,
                overflowWrap: 'anywhere',
              }}
            >
              {request.toolName}
            </code>
          ) : null}
        </div>

        {/* Scrollable content: prompt + request details */}
        <div
          style={{
            display: 'flex',
            minWidth: 0,
            minHeight: 0,
            flex: 1,
            flexDirection: 'column',
            gap: 10,
            maxHeight: INTERACTION_CONTENT_MAX_HEIGHT,
            overflowY: 'auto',
          }}
        >
          {prompt ? (
            <Text
              translate={kind === 'question' ? 'no' : undefined}
              style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}
            >
              {prompt}
            </Text>
          ) : null}

          <details style={{ minWidth: 0, maxWidth: '100%' }}>
            <summary
              className="aqbot-acp-interaction-summary"
              style={{ cursor: 'pointer', overflowWrap: 'anywhere' }}
            >
              {t('agentPage.interactionRequestDetails', '请求详情')}
            </summary>
            <pre
              style={{
                boxSizing: 'border-box',
                margin: '8px 0 0',
                maxHeight: 160,
                maxWidth: '100%',
                overflow: 'auto',
                padding: 8,
                borderRadius: token.borderRadius,
                background: token.colorFillQuaternary,
                whiteSpace: 'pre-wrap',
                overflowWrap: 'anywhere',
                wordBreak: 'break-word',
              }}
            >
              {inputJson}
            </pre>
          </details>
        </div>

        {/* Options stay fixed (not inside the scroll region) */}
        <div
          style={{
            display: 'flex',
            minWidth: 0,
            flexShrink: 0,
            flexWrap: 'wrap',
            gap: 8,
          }}
        >
          {displayOptions.map((option, index) => {
            const label = knownOptionLabel(kind, option, translate);
            const appearance = optionAppearance(kind, option);
            const optionLoading = loadingOptionId === option.id;
            const loadingLabel = t('agentPage.interactionSubmitting', '提交中…');
            const descriptionId = option.description ? `${titleId}-option-${index}` : undefined;
            return (
              <Button
                key={option.id}
                ref={index === 0 ? firstOptionRef : undefined}
                className="aqbot-acp-interaction-option"
                htmlType="button"
                translate={kind === 'question' ? 'no' : undefined}
                type={appearance.primary ? 'primary' : 'default'}
                danger={appearance.danger}
                disabled={submitting}
                aria-label={optionLoading ? `${label}，${loadingLabel}` : label}
                aria-describedby={descriptionId}
                onClick={() => void submitOption(option.id)}
                style={{ height: 'auto', maxWidth: '100%', paddingBlock: 6, textAlign: 'start' }}
              >
                <span style={{ display: 'flex', minWidth: 0, flexDirection: 'column' }}>
                  <span style={{ whiteSpace: 'normal', overflowWrap: 'anywhere' }}>{label}</span>
                  {optionLoading ? (
                    <span
                      style={{
                        color: 'inherit',
                        fontSize: 12,
                        fontWeight: 400,
                        opacity: 0.8,
                        whiteSpace: 'normal',
                      }}
                    >
                      {loadingLabel}
                    </span>
                  ) : null}
                  {option.description ? (
                    <span
                      id={descriptionId}
                      style={{
                        color: 'inherit',
                        fontSize: 12,
                        fontWeight: 400,
                        opacity: 0.72,
                        whiteSpace: 'normal',
                        overflowWrap: 'anywhere',
                      }}
                    >
                      {option.description}
                    </span>
                  ) : null}
                </span>
              </Button>
            );
          })}
        </div>

        {submissionError ? (
          <Text type="danger" role="alert" style={{ flexShrink: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
            {t('agentPage.interactionSubmitFailed', '提交失败，请重试')}: {submissionError}
          </Text>
        ) : null}
      </div>
    </form>
    </ConfigProvider>
  );
}
