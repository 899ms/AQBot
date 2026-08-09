import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { Button, ConfigProvider, Input, Modal, Typography, theme } from 'antd';
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
  active?: boolean;
}
export type AcpInteractionSubmission =
  | { optionId: string; feedback?: string }
  | { outcome: 'cancelled' }
  | { questionnaire: AcpQuestionnaireSubmission };
type Translate = (key: string) => string;

function normalizedToken(value: unknown): string {
  return String(value ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
}

function interactionTitle(kind: AcpInteractionKind, translate: Translate): string {
  if (kind === 'question') {
    return translate('agentPage.interactionQuestionTitle');
  }
  if (kind === 'plan_review') {
    return translate('agentPage.interactionPlanReviewTitle');
  }
  return translate('agentPage.interactionPermissionTitle');
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
      return translate('agentPage.interactionPlanExecute');
    }
    if (id === 'cancelled') {
      return translate('agentPage.interactionPlanRequestChanges');
    }
    if (id === 'abandoned') {
      return translate('agentPage.interactionPlanCancel');
    }
  }

  if (identity.includes('allowalways')) {
    return translate('agentPage.interactionAllowAlways');
  }
  if (identity.includes('allowonce') || id === 'approved' || id === 'approve') {
    return translate('agentPage.interactionAllowOnce');
  }
  if (
    identity.includes('reject')
    || identity.includes('deny')
    || identity.includes('cancel')
    || id === 'abandoned'
  ) {
    return translate('agentPage.interactionDeny');
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
  const value = [request.description, request.title]
    .find((candidate) => typeof candidate === 'string' && candidate.trim());
  return typeof value === 'string' ? value : null;
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

function planOptions(options: AcpInteractionOption[]): {
  approve?: AcpInteractionOption;
  change?: AcpInteractionOption;
  cancel?: AcpInteractionOption;
  additional: AcpInteractionOption[];
} {
  const byId = (ids: string[]) => options.find(
    (option) => ids.includes(normalizedToken(option.id)),
  );
  const approve = byId(['approved', 'approve', 'implementplan'])
    ?? options.find((option) => normalizedToken(option.kind).includes('allowonce'));
  const cancel = byId(['abandoned', 'abandon']);
  const change = byId(['cancelled', 'cancel', 'reviseplan'])
    ?? options.find((option) => (
      option !== cancel && normalizedToken(option.kind).includes('rejectonce')
    ));
  const selected = new Set([approve, change, cancel].filter(Boolean));
  return {
    approve,
    change,
    cancel,
    additional: options.filter((option) => !selected.has(option)),
  };
}

export function AcpInteractionComposer({
  request,
  onSubmit,
  active = true,
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
  }, [request.requestId]);

  useEffect(() => {
    if (!active) {
      setPlanExpanded(false);
      return undefined;
    }
    const frame = window.requestAnimationFrame(() => firstOptionRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [active, request.requestId]);

  if (request.status !== 'pending') return null;

  const kind = request.kind ?? 'permission';
  if (kind === 'question' && questionnaire) {
    return (
      <AcpQuestionnaireComposer
        active={active}
        request={request}
        questionnaire={questionnaire}
        onSubmit={(submission) => onSubmit({ questionnaire: submission })}
      />
    );
  }
  const translate: Translate = (key) => t(key);
  const title = interactionTitle(kind, translate);
  const prompt = promptText(request, kind);
  const inputJson = JSON.stringify(request.input ?? {}, null, 2);
  const submitting = loadingOptionId !== null;
  const displayOptions = request.options;

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

  const cancelInteraction = async () => {
    const requestId = request.requestId;
    setLoadingOptionId('cancelled');
    setSubmissionError(null);
    try {
      await onSubmit({ outcome: 'cancelled' });
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

  // ── Plan review: content in composer with max height + responsive actions ──
  if (kind === 'plan_review') {
    const {
      approve: approveOption,
      change: changeOption,
      cancel: cancelOption,
      additional: additionalPlanOptions,
    } = planOptions(request.options);
    const planBody = prompt ?? '';
    const supportsPlanFeedback = request.input?.supportsFeedback === true;
    const supportsNativeCancel = request.input?.feedbackDelivery === 'follow_up_prompt';
    const cancelActionId = cancelOption?.id ?? (supportsNativeCancel ? 'cancelled' : undefined);
    const firstPlanOptionId = approveOption?.id
      ?? changeOption?.id
      ?? cancelOption?.id
      ?? additionalPlanOptions[0]?.id
      ?? cancelActionId;
    const submitPlanFeedback = () => {
      if (!changeOption) return;
      const text = planFeedback.trim();
      if (!text) {
        setSubmissionError(t('agentPage.interactionPlanFeedbackRequired'));
        return;
      }
      void submitOption(changeOption.id, text);
    };

    const closePlanFullscreen = () => {
      setPlanExpanded(false);
    };

    const renderPlanForm = (expanded: boolean) => (
        <form
          aria-label={expanded ? title : undefined}
          aria-labelledby={expanded ? undefined : titleId}
          aria-busy={submitting}
          onSubmit={(event) => {
            event.preventDefault();
            if (planFeedbackMode) submitPlanFeedback();
          }}
          style={{
            display: 'flex',
            minWidth: 0,
            width: '100%',
            height: expanded ? '100%' : undefined,
            maxHeight: expanded ? '100%' : 'min(55vh, 480px)',
            flexDirection: 'column',
            gap: 10,
            touchAction: 'manipulation',
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
              {!expanded ? (
                <Text id={titleId} strong style={{ overflowWrap: 'anywhere' }}>
                  {title}
                </Text>
              ) : null}
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
              icon={expanded ? <Minimize2 size={16} /> : <Maximize2 size={16} />}
              aria-label={expanded
                ? t('agentPage.interactionPlanExitFullscreen')
                : t('agentPage.interactionPlanFullscreen')}
              aria-pressed={expanded}
              onClick={() => {
                if (expanded) closePlanFullscreen();
                else setPlanExpanded(true);
              }}
            />
          </div>

          {/* Scrollable plan body — same markdown stack as conversation bubbles */}
          {planBody ? (
            <div style={{ minWidth: 0, minHeight: 0, flex: 1, overflow: 'hidden', display: 'flex' }}>
              <AcpPlanMarkdownBody
                content={planBody}
                maxHeight={PLAN_DOCUMENT_MAX_HEIGHT}
                expanded={expanded}
              />
            </div>
          ) : (
            <Text type="secondary">{t('agentPage.interactionPlanEmpty')}</Text>
          )}

          {/* Fixed actions: never scrolled away */}
          {planFeedbackMode ? (
            <div style={{ display: 'flex', flexShrink: 0, flexDirection: 'column', gap: 8 }}>
              <Input.TextArea
                autoFocus
                value={planFeedback}
                disabled={submitting}
                rows={3}
                placeholder={t('agentPage.interactionPlanFeedbackPlaceholder')}
                aria-label={t('agentPage.interactionPlanFeedbackPlaceholder')}
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
                  {t('common.back')}
                </Button>
                <Button
                  type="primary"
                  disabled={submitting}
                  loading={loadingOptionId === changeOption?.id}
                  onClick={submitPlanFeedback}
                >
                  {t('agentPage.interactionPlanSubmitFeedback')}
                </Button>
              </div>
            </div>
          ) : (
            <div
              style={{
                display: 'grid',
                flexShrink: 0,
                gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 132px), 1fr))',
                gap: 8,
              }}
            >
              {approveOption ? (
                <Button
                  ref={!expanded && firstPlanOptionId === approveOption.id ? firstOptionRef : undefined}
                  className="aqbot-acp-interaction-option"
                  type="primary"
                  disabled={submitting}
                  loading={loadingOptionId === approveOption.id}
                  onClick={() => void submitOption(approveOption.id)}
                  style={{ height: 'auto', paddingBlock: 8, whiteSpace: 'normal' }}
                >
                  {t('agentPage.interactionPlanExecute')}
                </Button>
              ) : null}
              {changeOption ? (
                <Button
                  ref={!expanded && firstPlanOptionId === changeOption.id ? firstOptionRef : undefined}
                  className="aqbot-acp-interaction-option"
                  disabled={submitting}
                  loading={loadingOptionId === changeOption.id}
                  onClick={() => {
                    if (!supportsPlanFeedback) {
                      void submitOption(changeOption.id);
                      return;
                    }
                    setPlanFeedbackMode(true);
                    setSubmissionError(null);
                  }}
                  style={{ height: 'auto', paddingBlock: 8, whiteSpace: 'normal' }}
                >
                  {t('agentPage.interactionPlanRequestChanges')}
                </Button>
              ) : null}
              {cancelActionId ? (
                <Button
                  ref={!expanded && firstPlanOptionId === cancelActionId
                    ? firstOptionRef
                    : undefined}
                  className="aqbot-acp-interaction-option"
                  danger
                  disabled={submitting}
                  loading={loadingOptionId === cancelActionId}
                  onClick={() => {
                    if (cancelOption) void submitOption(cancelOption.id);
                    else void cancelInteraction();
                  }}
                  aria-label={t('agentPage.interactionPlanCancel')}
                  style={{ height: 'auto', paddingBlock: 8, whiteSpace: 'normal' }}
                >
                  {t('agentPage.interactionPlanCancel')}
                </Button>
              ) : null}
              {additionalPlanOptions.map((option) => {
                const appearance = optionAppearance('plan_review', option);
                return (
                  <Button
                    key={option.id}
                    ref={!expanded && firstPlanOptionId === option.id ? firstOptionRef : undefined}
                    className="aqbot-acp-interaction-option"
                    type={appearance.primary ? 'primary' : 'default'}
                    danger={appearance.danger}
                    disabled={submitting}
                    loading={loadingOptionId === option.id}
                    translate="no"
                    onClick={() => void submitOption(option.id)}
                    style={{ height: 'auto', paddingBlock: 8, whiteSpace: 'normal' }}
                  >
                    {option.label}
                  </Button>
                );
              })}
            </div>
          )}

          {submissionError ? (
            <Text type="danger" role="alert" style={{ flexShrink: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
              {t('agentPage.interactionSubmitFailed')}: {submissionError}
            </Text>
          ) : null}
        </form>
    );

    return (
      <ConfigProvider button={{ autoInsertSpace: false }}>
        <div
          aria-hidden={planExpanded || undefined}
          style={{ display: planExpanded ? 'none' : 'contents' }}
        >
          {renderPlanForm(false)}
        </div>
        <Modal
          open={planExpanded}
          title={title}
          footer={null}
          closable={false}
          keyboard
          mask={{ enabled: true, blur: true, closable: true }}
          onCancel={closePlanFullscreen}
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
          focusable={{ trap: true, focusTriggerAfterClose: true }}
          transitionName=""
          maskTransitionName=""
          destroyOnHidden
        >
          {planExpanded ? renderPlanForm(true) : null}
        </Modal>
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
              {t('agentPage.interactionRequestDetails')}
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
            const loadingLabel = t('agentPage.interactionSubmitting');
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
            {t('agentPage.interactionSubmitFailed')}: {submissionError}
          </Text>
        ) : null}
      </div>
    </form>
    </ConfigProvider>
  );
}
