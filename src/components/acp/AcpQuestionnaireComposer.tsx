import { useEffect, useId, useRef, useState } from 'react';
import { Button, Checkbox, ConfigProvider, Input, Radio, Typography, theme } from 'antd';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type {
  AcpPermissionRequest,
  AcpQuestionnaireAnswer,
  AcpQuestionnaireSubmission,
} from '@/stores/acpStore';

const { Text } = Typography;

/** Keep option list visible; only long descriptions/previews scroll. */
const QUESTION_CONTENT_MAX_HEIGHT = 280;
const OTHER_VALUE = '__aqbot_other__';

interface QuestionnaireOption {
  label: string;
  description?: string;
  preview?: string;
}

interface QuestionnaireQuestion {
  question: string;
  multiSelect: boolean;
  options: QuestionnaireOption[];
}

interface Questionnaire {
  questions: QuestionnaireQuestion[];
  mode: 'default' | 'plan';
}

interface AnswerDraft {
  selectedOptionIndexes: number[];
  otherSelected: boolean;
  otherText: string;
}

export interface AcpQuestionnaireComposerProps {
  request: AcpPermissionRequest;
  questionnaire: Questionnaire;
  onSubmit: (submission: AcpQuestionnaireSubmission) => Promise<void>;
  active?: boolean;
}

function optionalText(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

export function parseAcpQuestionnaire(
  input: Record<string, unknown>,
): Questionnaire | null {
  if (!Array.isArray(input.questions) || input.questions.length === 0) return null;
  const questions = input.questions.flatMap((entry): QuestionnaireQuestion[] => {
    if (!entry || typeof entry !== 'object') return [];
    const raw = entry as Record<string, unknown>;
    const question = optionalText(raw.question);
    if (!question) return [];
    const options = Array.isArray(raw.options)
      ? raw.options.flatMap((option): QuestionnaireOption[] => {
          if (!option || typeof option !== 'object') return [];
          const value = option as Record<string, unknown>;
          const label = optionalText(value.label);
          if (!label) return [];
          return [{
            label,
            description: optionalText(value.description),
            preview: optionalText(value.preview),
          }];
        })
      : [];
    return [{
      question,
      multiSelect: raw.multiSelect === true || raw.multi_select === true,
      options,
    }];
  });
  return questions.length === 0
    ? null
    : { questions, mode: input.mode === 'plan' ? 'plan' : 'default' };
}

function initialDrafts(questionnaire: Questionnaire): AnswerDraft[] {
  return questionnaire.questions.map(() => ({
    selectedOptionIndexes: [],
    otherSelected: false,
    otherText: '',
  }));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function answersFromDrafts(drafts: AnswerDraft[]): AcpQuestionnaireAnswer[] {
  return drafts.flatMap((entry, questionIndex) => {
    const otherText = entry.otherSelected && entry.otherText.trim()
      ? entry.otherText
      : '';
    if (entry.selectedOptionIndexes.length === 0 && !otherText) return [];
    return [{
      questionIndex,
      selectedOptionIndexes: entry.selectedOptionIndexes,
      ...(otherText ? { otherText } : {}),
    }];
  });
}

function OptionLabel({
  label,
  description,
  preview,
  showPreview,
  secondaryColor,
}: {
  label: string;
  description?: string;
  preview?: string;
  showPreview: boolean;
  secondaryColor: string;
}) {
  return (
    <span style={{ display: 'block', minWidth: 0, lineHeight: 1.45 }}>
      <span style={{ display: 'block', overflowWrap: 'anywhere' }}>{label}</span>
      {description ? (
        <span
          style={{
            display: 'block',
            marginTop: 2,
            color: secondaryColor,
            fontSize: 12,
            overflowWrap: 'anywhere',
          }}
        >
          {description}
        </span>
      ) : null}
      {showPreview && preview ? (
        <pre
          style={{
            maxHeight: 120,
            marginBlock: 6,
            overflow: 'auto',
            whiteSpace: 'pre-wrap',
            overflowWrap: 'anywhere',
          }}
        >
          {preview}
        </pre>
      ) : null}
    </span>
  );
}

export function AcpQuestionnaireComposer({
  request,
  questionnaire,
  onSubmit,
  active = true,
}: AcpQuestionnaireComposerProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const titleId = useId();
  const firstControlRef = useRef<HTMLElement | null>(null);
  const activeRequestIdRef = useRef(request.requestId);
  const mountedRef = useRef(true);
  const autoAdvanceTimerRef = useRef<number | null>(null);
  const [drafts, setDrafts] = useState(() => initialDrafts(questionnaire));
  const [currentIndex, setCurrentIndex] = useState(0);
  const [submitting, setSubmitting] = useState(false);
  const [submissionError, setSubmissionError] = useState<string | null>(null);
  const [validationError, setValidationError] = useState<string | null>(null);
  activeRequestIdRef.current = request.requestId;

  const total = questionnaire.questions.length;
  const safeIndex = Math.min(currentIndex, Math.max(0, total - 1));
  const question = questionnaire.questions[safeIndex];
  const draft = drafts[safeIndex]
    ?? { selectedOptionIndexes: [], otherSelected: false, otherText: '' };
  const isLast = safeIndex >= total - 1;
  const isFirst = safeIndex <= 0;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (autoAdvanceTimerRef.current !== null) {
        window.clearTimeout(autoAdvanceTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    setDrafts(initialDrafts(questionnaire));
    setCurrentIndex(0);
    setSubmitting(false);
    setSubmissionError(null);
    setValidationError(null);
  }, [request.requestId, questionnaire]);

  useEffect(() => {
    if (!active) return undefined;
    const frame = window.requestAnimationFrame(() => {
      firstControlRef.current?.focus?.();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [active, request.requestId, safeIndex]);

  const updateDraft = (questionIndex: number, update: (draft: AnswerDraft) => AnswerDraft) => {
    setDrafts((current) => current.map((entry, index) => (
      index === questionIndex ? update(entry) : entry
    )));
  };

  const submitWithDrafts = async (
    nextDrafts: AnswerDraft[],
    outcome: AcpQuestionnaireSubmission['outcome'],
  ) => {
    const requestId = request.requestId;
    const answers = answersFromDrafts(nextDrafts);
    if (outcome === 'accepted') {
      const emptyOther = nextDrafts.some((entry) => entry.otherSelected && !entry.otherText.trim());
      if (emptyOther || answers.length === 0) {
        setValidationError(t('agentPage.interactionAnswerRequired'));
        return;
      }
    }
    setSubmitting(true);
    setSubmissionError(null);
    setValidationError(null);
    try {
      await onSubmit({ outcome, answers });
    } catch (error) {
      if (mountedRef.current && activeRequestIdRef.current === requestId) {
        setSubmissionError(errorMessage(error));
      }
    } finally {
      if (mountedRef.current && activeRequestIdRef.current === requestId) setSubmitting(false);
    }
  };

  const advanceAfterSingleSelect = (questionIndex: number, nextDrafts: AnswerDraft[]) => {
    setValidationError(null);
    setSubmissionError(null);
    if (autoAdvanceTimerRef.current !== null) {
      window.clearTimeout(autoAdvanceTimerRef.current);
    }
    if (questionIndex >= total - 1) {
      void submitWithDrafts(nextDrafts, 'accepted');
      return;
    }
    // Brief pause so the selection is visible before flipping the page.
    autoAdvanceTimerRef.current = window.setTimeout(() => {
      autoAdvanceTimerRef.current = null;
      if (!mountedRef.current || activeRequestIdRef.current !== request.requestId) return;
      setCurrentIndex(questionIndex + 1);
    }, 180);
  };

  const selectOption = (questionIndex: number, optionIndex: number, multiSelect: boolean) => {
    if (multiSelect) {
      updateDraft(questionIndex, (entry) => {
        const selected = entry.selectedOptionIndexes.includes(optionIndex)
          ? entry.selectedOptionIndexes.filter((index) => index !== optionIndex)
          : [...entry.selectedOptionIndexes, optionIndex].sort((a, b) => a - b);
        return { ...entry, selectedOptionIndexes: selected };
      });
      return;
    }

    const nextDrafts = drafts.map((entry, index) => (
      index === questionIndex
        ? {
            selectedOptionIndexes: [optionIndex],
            otherSelected: false,
            otherText: entry.otherText,
          }
        : entry
    ));
    setDrafts(nextDrafts);
    advanceAfterSingleSelect(questionIndex, nextDrafts);
  };

  const selectOther = (questionIndex: number, multiSelect: boolean) => {
    updateDraft(questionIndex, (entry) => ({
      ...entry,
      selectedOptionIndexes: multiSelect ? entry.selectedOptionIndexes : [],
      otherSelected: multiSelect ? !entry.otherSelected : true,
    }));
  };

  const ensureOtherSelected = (questionIndex: number, multiSelect: boolean) => {
    updateDraft(questionIndex, (entry) => ({
      ...entry,
      selectedOptionIndexes: multiSelect ? entry.selectedOptionIndexes : [],
      otherSelected: true,
    }));
  };

  const goPrev = () => {
    if (autoAdvanceTimerRef.current !== null) {
      window.clearTimeout(autoAdvanceTimerRef.current);
      autoAdvanceTimerRef.current = null;
    }
    setSubmissionError(null);
    setValidationError(null);
    setCurrentIndex((index) => Math.max(0, index - 1));
  };

  const goNext = () => {
    if (autoAdvanceTimerRef.current !== null) {
      window.clearTimeout(autoAdvanceTimerRef.current);
      autoAdvanceTimerRef.current = null;
    }
    setSubmissionError(null);
    setValidationError(null);
    if (draft.otherSelected && !draft.otherText.trim()) {
      setValidationError(t('agentPage.interactionAnswerRequired'));
      return;
    }
    setCurrentIndex((index) => Math.min(total - 1, index + 1));
  };

  const submit = async (outcome: AcpQuestionnaireSubmission['outcome']) => {
    await submitWithDrafts(drafts, outcome);
  };

  if (!question) return null;

  const hint = question.multiSelect
    ? t('agentPage.interactionSelectMany')
    : t('agentPage.interactionSelectOne');

  const radioValue = draft.otherSelected
    ? OTHER_VALUE
    : draft.selectedOptionIndexes[0] ?? undefined;

  return (
    <ConfigProvider button={{ autoInsertSpace: false }}>
      <form
        aria-labelledby={titleId}
        aria-busy={submitting}
        onSubmit={(event) => {
          event.preventDefault();
          if (submitting) return;
          if (!isLast) {
            goNext();
            return;
          }
          void submit('accepted');
        }}
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
          .aqbot-acp-question-option.ant-radio-wrapper,
          .aqbot-acp-question-option.ant-checkbox-wrapper {
            display: flex !important;
            align-items: center;
            min-width: 0;
            margin-inline-end: 0 !important;
            white-space: normal;
          }
          .aqbot-acp-question-option .ant-radio,
          .aqbot-acp-question-option .ant-checkbox {
            align-self: center;
            top: 0;
          }
          .aqbot-acp-question-option span.ant-radio + span,
          .aqbot-acp-question-option span.ant-checkbox + span {
            min-width: 0;
            padding-inline-start: 8px;
          }
          .aqbot-acp-question-summary:focus-visible {
            outline: 2px solid ${token.colorPrimaryBorder};
            outline-offset: 2px;
          }
        `}</style>

        {/* Header: title + progress / nav */}
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
          <div style={{ display: 'flex', minWidth: 0, flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
            <Text id={titleId} strong>{t('agentPage.interactionQuestionTitle')}</Text>
            <code
              translate="no"
              style={{
                padding: '1px 4px',
                borderRadius: token.borderRadiusSM,
                background: token.colorFillQuaternary,
              }}
            >
              {request.toolName}
            </code>
          </div>
          {total > 1 ? (
            <div
              style={{
                display: 'inline-flex',
                flexShrink: 0,
                alignItems: 'center',
                gap: 4,
              }}
            >
              <Button
                type="text"
                size="small"
                disabled={submitting || isFirst}
                icon={<ChevronLeft size={16} />}
                aria-label={t('agentPage.interactionPrevQuestion')}
                onClick={goPrev}
              />
              <Text
                type="secondary"
                aria-live="polite"
                style={{ minWidth: 40, textAlign: 'center', fontSize: 12, fontVariantNumeric: 'tabular-nums' }}
              >
                {safeIndex + 1}/{total}
              </Text>
              <Button
                type="text"
                size="small"
                disabled={submitting || isLast}
                icon={<ChevronRight size={16} />}
                aria-label={t('agentPage.interactionNextQuestion')}
                onClick={goNext}
              />
            </div>
          ) : null}
        </div>

        {/* Single question body */}
        <fieldset
          disabled={submitting}
          translate="no"
          style={{
            display: 'flex',
            minWidth: 0,
            minHeight: 0,
            flex: 1,
            flexDirection: 'column',
            gap: 8,
            margin: 0,
            padding: 10,
            border: `1px solid ${token.colorBorderSecondary}`,
            borderRadius: token.borderRadius,
            overflow: 'hidden',
          }}
        >
          <legend translate="no" style={{ maxWidth: '100%', paddingInline: 4, overflowWrap: 'anywhere' }}>
            {question.question}
          </legend>
          <Text type="secondary" style={{ display: 'block', flexShrink: 0, fontSize: 12 }}>
            {hint}
          </Text>

          <div
            style={{
              display: 'flex',
              minWidth: 0,
              minHeight: 0,
              flex: 1,
              flexDirection: 'column',
              gap: 10,
              maxHeight: QUESTION_CONTENT_MAX_HEIGHT,
              overflowY: 'auto',
            }}
          >
            {question.multiSelect ? (
              <>
                <Checkbox.Group
                  value={draft.selectedOptionIndexes.map(String)}
                  disabled={submitting}
                  style={{ display: 'flex', flexDirection: 'column', gap: 10, width: '100%' }}
                  onChange={(values) => {
                    const selectedOptionIndexes = values
                      .map(String)
                      .map(Number)
                      .filter((index) => Number.isInteger(index) && index >= 0)
                      .sort((a, b) => a - b);
                    updateDraft(safeIndex, (entry) => ({
                      ...entry,
                      selectedOptionIndexes,
                    }));
                  }}
                >
                  {question.options.map((option, optionIndex) => {
                    const checked = draft.selectedOptionIndexes.includes(optionIndex);
                    return (
                      <Checkbox
                        key={`${optionIndex}-${option.label}`}
                        ref={optionIndex === 0
                          ? (node) => {
                              firstControlRef.current = node as unknown as HTMLElement | null;
                            }
                          : undefined}
                        className="aqbot-acp-question-option"
                        value={String(optionIndex)}
                        style={{ width: '100%' }}
                      >
                        <OptionLabel
                          label={option.label}
                          description={option.description}
                          preview={option.preview}
                          showPreview={checked}
                          secondaryColor={token.colorTextSecondary}
                        />
                      </Checkbox>
                    );
                  })}
                </Checkbox.Group>
                <div style={{ display: 'flex', minWidth: 0, alignItems: 'center', gap: 8, width: '100%' }}>
                  <Checkbox
                    ref={question.options.length === 0
                      ? (node) => {
                          firstControlRef.current = node as unknown as HTMLElement | null;
                        }
                      : undefined}
                    className="aqbot-acp-question-option"
                    checked={draft.otherSelected}
                    disabled={submitting}
                    onChange={(event) => {
                      const checked = event.target.checked;
                      updateDraft(safeIndex, (entry) => ({
                        ...entry,
                        otherSelected: checked,
                      }));
                    }}
                  >
                    {t('agentPage.interactionOther')}
                  </Checkbox>
                  <Input
                    value={draft.otherText}
                    disabled={submitting}
                    aria-label={`${t('agentPage.interactionOther')}: ${question.question}`}
                    onFocus={() => ensureOtherSelected(safeIndex, true)}
                    onChange={(event) => updateDraft(safeIndex, (current) => ({
                      ...current,
                      otherSelected: true,
                      otherText: event.target.value,
                    }))}
                    style={{ minWidth: 0, flex: 1 }}
                  />
                </div>
              </>
            ) : (
              <Radio.Group
                value={radioValue}
                disabled={submitting}
                style={{ display: 'flex', flexDirection: 'column', gap: 10, width: '100%' }}
                onChange={(event) => {
                  const value = event.target.value;
                  if (value === OTHER_VALUE) {
                    selectOther(safeIndex, false);
                    return;
                  }
                  selectOption(safeIndex, Number(value), false);
                }}
              >
                {question.options.map((option, optionIndex) => {
                  const checked = draft.selectedOptionIndexes.includes(optionIndex);
                  return (
                    <Radio
                      key={`${optionIndex}-${option.label}`}
                      ref={optionIndex === 0
                        ? (node) => {
                            firstControlRef.current = node as unknown as HTMLElement | null;
                          }
                        : undefined}
                      className="aqbot-acp-question-option"
                      value={optionIndex}
                      style={{ width: '100%' }}
                    >
                      <OptionLabel
                        label={option.label}
                        description={option.description}
                        preview={option.preview}
                        showPreview={checked}
                        secondaryColor={token.colorTextSecondary}
                      />
                    </Radio>
                  );
                })}
                <div style={{ display: 'flex', minWidth: 0, alignItems: 'center', gap: 8, width: '100%' }}>
                  <Radio
                    ref={question.options.length === 0
                      ? (node) => {
                          firstControlRef.current = node as unknown as HTMLElement | null;
                        }
                      : undefined}
                    className="aqbot-acp-question-option"
                    value={OTHER_VALUE}
                  >
                    {t('agentPage.interactionOther')}
                  </Radio>
                  <Input
                    value={draft.otherText}
                    disabled={submitting}
                    aria-label={`${t('agentPage.interactionOther')}: ${question.question}`}
                    onFocus={() => ensureOtherSelected(safeIndex, false)}
                    onChange={(event) => updateDraft(safeIndex, (current) => ({
                      ...current,
                      selectedOptionIndexes: [],
                      otherSelected: true,
                      otherText: event.target.value,
                    }))}
                    style={{ minWidth: 0, flex: 1 }}
                  />
                </div>
              </Radio.Group>
            )}
          </div>
        </fieldset>

        <details style={{ minWidth: 0, flexShrink: 0 }}>
          <summary className="aqbot-acp-question-summary" style={{ cursor: 'pointer' }}>
            {t('agentPage.interactionRequestDetails')}
          </summary>
          <pre style={{ maxHeight: 120, overflow: 'auto', whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
            {JSON.stringify(request.input, null, 2)}
          </pre>
        </details>

        {/* Actions stay pinned below options */}
        <div
          style={{
            display: 'flex',
            flexShrink: 0,
            flexWrap: 'wrap',
            justifyContent: 'flex-end',
            gap: 8,
          }}
        >
          <Button disabled={submitting} onClick={() => void submit('cancelled')}>
            {t('common.cancel')}
          </Button>
          {questionnaire.mode === 'plan' ? (
            <>
              <Button disabled={submitting} onClick={() => void submit('chat_about_this')}>
                {t('agentPage.interactionChatAboutThis')}
              </Button>
              <Button disabled={submitting} onClick={() => void submit('skip_interview')}>
                {t('agentPage.interactionSkipInterview')}
              </Button>
            </>
          ) : null}
          {!isLast ? (
            <Button type="primary" htmlType="submit" disabled={submitting}>
              {t('agentPage.interactionContinue')}
            </Button>
          ) : (
            <Button type="primary" htmlType="submit" disabled={submitting}>
              {submitting
                ? t('agentPage.interactionSubmitting')
                : t('agentPage.interactionSubmitAnswers')}
            </Button>
          )}
        </div>

        {validationError ? (
          <Text type="danger" role="alert" style={{ flexShrink: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
            {validationError}
          </Text>
        ) : null}
        {submissionError ? (
          <Text type="danger" role="alert" style={{ flexShrink: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
            {t('agentPage.interactionSubmitFailed')}: {submissionError}
          </Text>
        ) : null}
      </form>
    </ConfigProvider>
  );
}
