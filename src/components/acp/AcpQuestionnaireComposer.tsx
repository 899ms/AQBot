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
  value?: string;
  label: string;
  description?: string;
  preview?: string;
}

interface QuestionnaireQuestion {
  id?: string;
  title?: string;
  question: string;
  description?: string;
  required: boolean;
  allowOther: boolean;
  inputType: string;
  format?: string;
  secret: boolean;
  defaultValue?: unknown;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  minimum?: number;
  maximum?: number;
  minItems?: number;
  maxItems?: number;
  multiSelect: boolean;
  options: QuestionnaireOption[];
}

interface Questionnaire {
  questions: QuestionnaireQuestion[];
  mode: 'default' | 'plan';
  standardForm: boolean;
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

function optionalNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

export function parseAcpQuestionnaire(
  input: Record<string, unknown>,
): Questionnaire | null {
  if (!Array.isArray(input.questions) || input.questions.length === 0) return null;
  const normalizedForm = input.kind === 'elicitation_form';
  const questions = input.questions.flatMap((entry): QuestionnaireQuestion[] => {
    if (!entry || typeof entry !== 'object') return [];
    const raw = entry as Record<string, unknown>;
    const question = optionalText(raw.question);
    if (!question) return [];
    const rawInputType = optionalText(raw.inputType) ?? 'text';
    const format = optionalText(raw.format);
    const inputType = (rawInputType === 'string' || rawInputType === 'text')
      && ['email', 'uri', 'date', 'date-time'].includes(format ?? '')
      ? format!
      : rawInputType;
    const options = Array.isArray(raw.options)
      ? raw.options.flatMap((option): QuestionnaireOption[] => {
          if (!option || typeof option !== 'object') return [];
          const value = option as Record<string, unknown>;
          const label = optionalText(value.label);
          if (!label) return [];
          return [{
            value: optionalText(value.value),
            label,
            description: optionalText(value.description),
            preview: optionalText(value.preview),
          }];
        })
      : [];
    return [{
      id: optionalText(raw.id),
      title: optionalText(raw.title),
      question,
      description: optionalText(raw.description),
      required: raw.required === true,
      allowOther: raw.allowOther === true || (!normalizedForm && raw.allowOther !== false),
      inputType,
      format,
      secret: raw.secret === true || raw.inputType === 'secret',
      ...(raw.default !== undefined ? { defaultValue: raw.default } : {}),
      minLength: optionalNumber(raw.minLength),
      maxLength: optionalNumber(raw.maxLength),
      pattern: optionalText(raw.pattern),
      minimum: optionalNumber(raw.minimum),
      maximum: optionalNumber(raw.maximum),
      minItems: optionalNumber(raw.minItems),
      maxItems: optionalNumber(raw.maxItems),
      multiSelect: raw.multiSelect === true || raw.multi_select === true,
      options,
    }];
  });
  return questions.length === 0
    ? null
    : {
        questions,
        mode: input.mode === 'plan' ? 'plan' : 'default',
        standardForm: normalizedForm,
      };
}

function localDateTimeValue(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const localDate = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return localDate.toISOString().slice(0, 16);
}

function initialDrafts(questionnaire: Questionnaire): AnswerDraft[] {
  return questionnaire.questions.map((question) => {
    const defaults = Array.isArray(question.defaultValue)
      ? question.defaultValue.map(String)
      : question.defaultValue === undefined || question.defaultValue === null
        ? []
        : [String(question.defaultValue)];
    const selectedOptionIndexes = question.options.flatMap((option, index) => (
      option.value !== undefined && defaults.includes(option.value) ? [index] : []
    ));
    const rawDirectDefault = !question.secret && question.options.length === 0
      ? defaults[0] ?? ''
      : '';
    const directDefault = question.inputType === 'date-time' && rawDirectDefault
      ? localDateTimeValue(rawDirectDefault)
      : rawDirectDefault;
    return {
      selectedOptionIndexes,
      otherSelected: !!directDefault,
      otherText: directDefault,
    };
  });
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function normalizedAnswerText(
  question: QuestionnaireQuestion | undefined,
  value: string,
): string {
  if (question?.inputType !== 'date-time') return value;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toISOString();
}

function answersFromDrafts(
  drafts: AnswerDraft[],
  questions: QuestionnaireQuestion[],
): AcpQuestionnaireAnswer[] {
  return drafts.flatMap((entry, questionIndex) => {
    const rawOtherText = entry.otherSelected && entry.otherText.trim()
      ? entry.otherText
      : '';
    const otherText = rawOtherText
      ? normalizedAnswerText(questions[questionIndex], rawOtherText)
      : '';
    if (entry.selectedOptionIndexes.length === 0 && !otherText) return [];
    return [{
      questionIndex,
      selectedOptionIndexes: entry.selectedOptionIndexes,
      ...(otherText ? { otherText } : {}),
    }];
  });
}

type QuestionValidationIssue = 'required' | 'invalid' | 'unsupported';

const SUPPORTED_INPUT_TYPES = new Set([
  'string', 'text', 'secret', 'email', 'uri', 'date', 'date-time',
  'integer', 'number', 'boolean', 'array',
]);

function validateQuestionDraft(
  question: QuestionnaireQuestion,
  draft: AnswerDraft | undefined,
): QuestionValidationIssue | null {
  if (!SUPPORTED_INPUT_TYPES.has(question.inputType)) return 'unsupported';
  const selectedCount = draft?.selectedOptionIndexes.length ?? 0;
  const otherText = draft?.otherText.trim() ?? '';
  if (question.required && selectedCount === 0 && !otherText) return 'required';
  if (!otherText) {
    if (question.minItems !== undefined && selectedCount < question.minItems) return 'invalid';
    return question.maxItems !== undefined && selectedCount > question.maxItems ? 'invalid' : null;
  }
  if (question.inputType === 'integer' || question.inputType === 'number') {
    const numeric = Number(otherText);
    if (!Number.isFinite(numeric)) return 'invalid';
    if (question.inputType === 'integer' && !Number.isInteger(numeric)) return 'invalid';
    if (question.minimum !== undefined && numeric < question.minimum) return 'invalid';
    if (question.maximum !== undefined && numeric > question.maximum) return 'invalid';
  }
  if (question.inputType === 'email' && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(otherText)) {
    return 'invalid';
  }
  if (question.inputType === 'uri') {
    try {
      // ACP's URI format requires an absolute URI rather than a relative path.
      if (!new URL(otherText).protocol) return 'invalid';
    } catch {
      return 'invalid';
    }
  }
  if (question.inputType === 'date') {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(otherText) || Number.isNaN(Date.parse(otherText))) {
      return 'invalid';
    }
  }
  if (question.inputType === 'date-time' && Number.isNaN(Date.parse(otherText))) {
    return 'invalid';
  }
  if (question.minLength !== undefined && otherText.length < question.minLength) return 'invalid';
  if (question.maxLength !== undefined && otherText.length > question.maxLength) return 'invalid';
  if (question.pattern) {
    try {
      if (!new RegExp(question.pattern).test(otherText)) return 'invalid';
    } catch {
      return 'unsupported';
    }
  }
  const itemCount = selectedCount + (draft?.otherSelected && otherText ? 1 : 0);
  if (question.minItems !== undefined && itemCount < question.minItems) return 'invalid';
  if (question.maxItems !== undefined && itemCount > question.maxItems) return 'invalid';
  return null;
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
  const hasUnsupportedQuestion = questionnaire.questions.some(
    (entry) => !SUPPORTED_INPUT_TYPES.has(entry.inputType),
  );

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
    const answers = answersFromDrafts(nextDrafts, questionnaire.questions);
    if (outcome === 'accepted') {
      if (!questionnaire.standardForm && answers.length === 0) {
        setValidationError(t('agentPage.interactionAnswerRequired'));
        return;
      }
      const invalidIndex = questionnaire.questions.findIndex(
        (entry, index) => validateQuestionDraft(entry, nextDrafts[index]) !== null,
      );
      if (invalidIndex >= 0) {
        const issue = validateQuestionDraft(
          questionnaire.questions[invalidIndex],
          nextDrafts[invalidIndex],
        );
        setCurrentIndex(invalidIndex);
        setValidationError(t(issue === 'required'
          ? 'agentPage.interactionAnswerRequired'
          : issue === 'unsupported'
            ? 'agentPage.interactionUnsupportedField'
            : 'agentPage.interactionAnswerInvalid'));
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
    const issue = validateQuestionDraft(question, draft);
    if (issue) {
      setValidationError(t(issue === 'required'
        ? 'agentPage.interactionAnswerRequired'
        : issue === 'unsupported'
          ? 'agentPage.interactionUnsupportedField'
          : 'agentPage.interactionAnswerInvalid'));
      return;
    }
    setCurrentIndex((index) => Math.min(total - 1, index + 1));
  };

  const submit = async (outcome: AcpQuestionnaireSubmission['outcome']) => {
    await submitWithDrafts(drafts, outcome);
  };

  if (!question) return null;

  const hint = questionnaire.standardForm && question.options.length === 0
    ? t('agentPage.interactionEnterAnswer')
    : question.multiSelect
      ? t('agentPage.interactionSelectMany')
      : t('agentPage.interactionSelectOne');

  const radioValue = draft.otherSelected
    ? OTHER_VALUE
    : draft.selectedOptionIndexes[0] ?? undefined;
  const OtherAnswerInput = question.secret ? Input.Password : Input;
  const optionFocusInset = token.lineWidthFocus + 1;

  return (
    <ConfigProvider button={{ autoInsertSpace: false }}>
      <form
        aria-labelledby={titleId}
        aria-busy={submitting}
        noValidate
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
            {question.title ?? question.question}
          </legend>
          {question.title && question.title !== question.question ? (
            <Text translate="no" style={{ display: 'block', flexShrink: 0, overflowWrap: 'anywhere' }}>
              {question.question}
            </Text>
          ) : null}
          {question.description && question.description !== question.question ? (
            <Text
              type="secondary"
              translate="no"
              style={{ display: 'block', flexShrink: 0, fontSize: 12, overflowWrap: 'anywhere' }}
            >
              {question.description}
            </Text>
          ) : null}
          <Text type="secondary" style={{ display: 'block', flexShrink: 0, fontSize: 12 }}>
            {hint}
          </Text>

          <div
            style={{
              display: 'flex',
              boxSizing: 'border-box',
              minWidth: 0,
              minHeight: 0,
              flex: 1,
              flexDirection: 'column',
              gap: 10,
              maxHeight: QUESTION_CONTENT_MAX_HEIGHT,
              overflowY: 'auto',
              padding: optionFocusInset,
            }}
          >
            {!SUPPORTED_INPUT_TYPES.has(question.inputType) ? (
              <Text type="danger" role="alert">
                {t('agentPage.interactionUnsupportedField', { type: question.inputType })}
              </Text>
            ) : questionnaire.standardForm && question.options.length === 0 ? (
              question.secret ? (
                <Input.Password
                  ref={(node) => {
                    firstControlRef.current = node?.input ?? null;
                  }}
                  value={draft.otherText}
                  disabled={submitting}
                  minLength={question.minLength}
                  maxLength={question.maxLength}
                  pattern={question.pattern}
                  aria-label={question.title ?? question.question}
                  onChange={(event) => updateDraft(safeIndex, (entry) => ({
                    ...entry,
                    otherSelected: true,
                    otherText: event.target.value,
                  }))}
                />
              ) : (
                <Input
                  ref={(node) => {
                    firstControlRef.current = node?.input ?? null;
                  }}
                  value={draft.otherText}
                  disabled={submitting}
                  type={question.inputType === 'integer' || question.inputType === 'number'
                    ? 'number'
                    : question.inputType === 'email'
                      ? 'email'
                      : question.inputType === 'uri'
                        ? 'url'
                        : question.inputType === 'date'
                          ? 'date'
                          : question.inputType === 'date-time'
                            ? 'datetime-local'
                            : 'text'}
                  min={question.minimum}
                  max={question.maximum}
                  step={question.inputType === 'integer' ? 1 : question.inputType === 'number' ? 'any' : undefined}
                  minLength={question.minLength}
                  maxLength={question.maxLength}
                  pattern={question.pattern}
                  aria-label={question.title ?? question.question}
                  onChange={(event) => updateDraft(safeIndex, (entry) => ({
                    ...entry,
                    otherSelected: true,
                    otherText: event.target.value,
                  }))}
                />
              )
            ) : question.multiSelect ? (
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
                {question.allowOther ? (
                  <div
                    style={{ display: 'flex', minWidth: 0, alignItems: 'center', gap: 8, width: '100%' }}
                  >
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
                    <OtherAnswerInput
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
                ) : null}
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
                {question.allowOther ? (
                  <div
                    style={{ display: 'flex', minWidth: 0, alignItems: 'center', gap: 8, width: '100%' }}
                  >
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
                    <OtherAnswerInput
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
                ) : null}
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
          {questionnaire.standardForm ? (
            <Button disabled={submitting} onClick={() => void submit('declined')}>
              {t('agentPage.interactionDeclineAnswers')}
            </Button>
          ) : null}
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
            <Button type="primary" htmlType="submit" disabled={submitting || hasUnsupportedQuestion}>
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
