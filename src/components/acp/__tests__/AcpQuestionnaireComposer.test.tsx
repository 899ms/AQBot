import { App } from 'antd';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { translateZhCN } from '@/test/i18nTestTranslator';
import {
  AcpInteractionComposer,
  type AcpInteractionRequest,
  type AcpInteractionSubmission,
} from '../AcpInteractionComposer';

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => ({
    t: translateZhCN,
  }),
}));

const questionnaireRequest: AcpInteractionRequest = {
  threadId: 'thread-1',
  messageId: 'assistant-1',
  requestId: 'questionnaire-1',
  toolCallId: 'tool-questionnaire-1',
  toolName: 'ask_user_question',
  kind: 'question',
  status: 'pending',
  options: [],
  input: {
    mode: 'default',
    questions: [
      {
        id: 'store',
        question: 'Which store?',
        multiSelect: false,
        options: [
          { id: 'sqlite', label: 'SQLite', description: 'Local file' },
          { id: 'postgres', label: 'Postgres', preview: 'CREATE TABLE events (...);' },
        ],
      },
      {
        question: 'Which layers?',
        multiSelect: true,
        options: [
          { label: 'Frontend' },
          { label: 'Backend' },
        ],
      },
      {
        question: 'Anything else?',
        options: [],
      },
    ],
  },
};

function renderQuestionnaire(
  request: AcpInteractionRequest = questionnaireRequest,
  onSubmit: (submission: AcpInteractionSubmission) => Promise<void> = vi.fn(async () => undefined),
) {
  render(
    <App>
      <AcpInteractionComposer request={request} onSubmit={onSubmit} />
    </App>,
  );
}

function goNext() {
  // Prefer header nav (stable aria-label); footer may get CJK spacing from antd.
  const headerNext = screen.queryByRole('button', { name: '下一题' });
  if (headerNext && !headerNext.hasAttribute('disabled')) {
    fireEvent.click(headerNext);
    return;
  }
  fireEvent.click(screen.getByRole('button', { name: /继\s*续/ }));
}

describe('AcpQuestionnaireComposer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows one question at a time with progress navigation', async () => {
    renderQuestionnaire();

    expect(screen.getByText('Which store?')).toBeInTheDocument();
    expect(screen.queryByText('Which layers?')).not.toBeInTheDocument();
    expect(screen.getByText('1/3')).toBeInTheDocument();

    await waitFor(() => expect(screen.getByRole('radio', { name: /SQLite/i })).toBeInTheDocument());
    // Use header next without selecting (manual nav still works)
    fireEvent.click(screen.getByRole('button', { name: '下一题' }));

    expect(screen.getByText('Which layers?')).toBeInTheDocument();
    expect(screen.queryByText('Which store?')).not.toBeInTheDocument();
    expect(screen.getByText('2/3')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '上一题' }));
    expect(screen.getByText('Which store?')).toBeInTheDocument();
  });

  it('keeps radio and checkbox focus outlines inside the clipped option viewport', () => {
    renderQuestionnaire();

    const radio = screen.getByRole('radio', { name: /SQLite/i });
    const radioViewport = radio.closest('.ant-radio-group')?.parentElement;
    expect(radioViewport).toHaveStyle({
      boxSizing: 'border-box',
      overflowY: 'auto',
      padding: '4px',
    });

    fireEvent.click(screen.getByRole('button', { name: '下一题' }));

    const checkbox = screen.getByRole('checkbox', { name: 'Frontend' });
    const checkboxViewport = checkbox.closest('.ant-checkbox-group')?.parentElement;
    expect(checkboxViewport).toHaveStyle({
      boxSizing: 'border-box',
      overflowY: 'auto',
      padding: '4px',
    });
  });

  it('renders a normalized Codex enum without inventing an Other answer', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'codex-elicitation-enum',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'scope',
          title: 'Work scope',
          question: 'Which area should the plan cover?',
          description: 'Choose the narrowest useful scope.',
          required: true,
          inputType: 'text',
          default: 'toolbar',
          allowOther: false,
          options: [
            { value: 'toolbar', label: 'Toolbar only', description: 'Only change the toolbar.' },
            { value: 'full-app', label: 'Entire app', description: 'Cover every surface.' },
          ],
        }],
      },
    }, onSubmit);

    expect(screen.getByText('Work scope')).toBeInTheDocument();
    expect(screen.getByText('Which area should the plan cover?')).toBeInTheDocument();
    expect(screen.getByText('Choose the narrowest useful scope.')).toBeInTheDocument();
    expect(screen.queryByText('其他')).not.toBeInTheDocument();
    expect(screen.getByRole('radio', { name: /Toolbar only/i })).toBeChecked();

    fireEvent.click(screen.getByRole('radio', { name: /Entire app/i }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [{ questionIndex: 0, selectedOptionIndexes: [1] }],
        },
      });
    });
  });

  it('requires normalized secret input, masks it, and never shows it in request details', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'codex-elicitation-secret',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'api_token',
          title: 'API token',
          question: 'Enter the temporary API token.',
          required: true,
          inputType: 'secret',
          secret: true,
          allowOther: false,
          options: [],
        }],
      },
    }, onSubmit);

    const secret = screen.getByLabelText('API token');
    expect(secret).toHaveAttribute('type', 'password');
    expect(screen.queryByText('其他')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('请回答此问题');
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(secret, { target: { value: 'super-secret-value' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [{
            questionIndex: 0,
            selectedOptionIndexes: [],
            otherText: 'super-secret-value',
          }],
        },
      });
    });
    expect(screen.getByText('请求详情').closest('details')).not.toHaveTextContent('super-secret-value');
  });

  it('applies normalized integer defaults and bounds before submitting', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'codex-elicitation-integer',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'variant_count',
          title: 'Variant count',
          question: 'How many variants should be compared?',
          required: true,
          inputType: 'integer',
          default: 2,
          minimum: 1,
          maximum: 5,
          allowOther: false,
          options: [],
        }],
      },
    }, onSubmit);

    const count = screen.getByRole('spinbutton', { name: 'Variant count' });
    expect(count).toHaveValue(2);
    expect(count).toHaveAttribute('min', '1');
    expect(count).toHaveAttribute('max', '5');
    expect(count).toHaveAttribute('step', '1');

    fireEvent.change(count, { target: { value: '6' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(count, { target: { value: '3' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [{
            questionIndex: 0,
            selectedOptionIndexes: [],
            otherText: '3',
          }],
        },
      });
    });
  });

  it('enforces normalized array bounds using stable option values', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'standard-form-array',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'layers',
          title: 'Layers',
          question: 'Which layers should change?',
          required: true,
          inputType: 'array',
          default: ['frontend'],
          minItems: 2,
          maxItems: 2,
          multiSelect: true,
          allowOther: false,
          options: [
            { value: 'frontend', label: 'Frontend' },
            { value: 'backend', label: 'Backend' },
            { value: 'database', label: 'Database' },
          ],
        }],
      },
    }, onSubmit);

    expect(screen.getByRole('checkbox', { name: 'Frontend' })).toBeChecked();
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('checkbox', { name: 'Backend' }));
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [{ questionIndex: 0, selectedOptionIndexes: [0, 1] }],
        },
      });
    });
  });

  it('uses a password field for an explicitly allowed secret Other answer', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'standard-form-secret-other',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'credential',
          title: 'Credential',
          question: 'Which credential should be used?',
          required: true,
          inputType: 'secret',
          secret: true,
          allowOther: true,
          options: [{ value: 'keychain', label: 'Use keychain' }],
        }],
      },
    }, onSubmit);

    const other = screen.getByLabelText('其他: Which credential should be used?');
    expect(other).toHaveAttribute('type', 'password');
    fireEvent.change(other, { target: { value: 'temporary-secret' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [{
            questionIndex: 0,
            selectedOptionIndexes: [],
            otherText: 'temporary-secret',
          }],
        },
      });
    });
    expect(screen.getByText('请求详情').closest('details')).not.toHaveTextContent('temporary-secret');
  });

  it('shows an unsupported normalized field without pretending it is a text input', () => {
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'future-elicitation-field',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'future',
          title: 'Future field',
          question: 'Use the future control.',
          required: false,
          inputType: 'future-widget',
          allowOther: false,
          options: [],
        }],
      },
    });

    expect(screen.getByRole('alert')).toHaveTextContent('future-widget');
    expect(screen.queryByRole('textbox', { name: 'Future field' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: /提\s*交回答/ })).toBeDisabled();
  });

  it('keeps standard form decline separate from cancelling the interaction', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'standard-form-decline',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'optional_note',
          title: 'Optional note',
          question: 'Add an optional note.',
          required: false,
          inputType: 'text',
          allowOther: false,
          options: [],
        }],
      },
    }, onSubmit);

    expect(screen.getByRole('button', { name: '拒绝回答' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '取消' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '拒绝回答' }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: { outcome: 'declined', answers: [] },
      });
    });
  });

  it('uses normalized string format and constraints for free-form input', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'standard-form-email',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'contact',
          title: 'Contact email',
          question: 'Where should the summary be sent?',
          required: true,
          inputType: 'string',
          format: 'email',
          minLength: 6,
          maxLength: 80,
          pattern: '^[^@]+@[^@]+\\.[^@]+$',
          allowOther: false,
          options: [],
        }],
      },
    }, onSubmit);

    const email = screen.getByRole('textbox', { name: 'Contact email' });
    expect(email).toHaveAttribute('type', 'email');
    expect(email).toHaveAttribute('minlength', '6');
    expect(email).toHaveAttribute('maxlength', '80');
    expect(email).toHaveAttribute('pattern', '^[^@]+@[^@]+\\.[^@]+$');

    fireEvent.change(email, { target: { value: 'invalid' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(email, { target: { value: 'team@example.com' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
  });

  it('rejects an invalid normalized URI before sending it to the agent', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'standard-form-uri',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'docs_url',
          title: 'Documentation URL',
          question: 'Which documentation should be used?',
          required: true,
          inputType: 'string',
          format: 'uri',
          allowOther: false,
          options: [],
        }],
      },
    }, onSubmit);

    const uri = screen.getByRole('textbox', { name: 'Documentation URL' });
    fireEvent.change(uri, { target: { value: 'not a URL' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.change(uri, { target: { value: 'https://example.com/docs' } });
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
  });

  it('converts datetime-local answers to RFC 3339 before submission', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'standard-form-date-time',
      input: {
        kind: 'elicitation_form',
        questions: [{
          id: 'deadline',
          title: 'Deadline',
          question: 'When should this be finished?',
          required: true,
          inputType: 'string',
          format: 'date-time',
          default: '2026-08-09T06:30:00.000Z',
          allowOther: false,
          options: [],
        }],
      },
    }, onSubmit);

    const defaultValue = '2026-08-09T06:30:00.000Z';
    const defaultDate = new Date(defaultValue);
    const expectedLocalValue = new Date(
      defaultDate.getTime() - defaultDate.getTimezoneOffset() * 60_000,
    ).toISOString().slice(0, 16);
    const deadline = screen.getByLabelText('Deadline');
    expect(deadline).toHaveAttribute('type', 'datetime-local');
    expect(deadline).toHaveValue(expectedLocalValue);
    fireEvent.click(screen.getByRole('button', { name: /提\s*交回答/ }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [{
            questionIndex: 0,
            selectedOptionIndexes: [],
            otherText: defaultValue,
          }],
        },
      });
    });
  });

  it('auto-advances after a single-select choice', async () => {
    renderQuestionnaire();

    fireEvent.click(screen.getByRole('radio', { name: /Postgres/i }));
    expect(await screen.findByText('Which layers?', {}, { timeout: 1500 })).toBeInTheDocument();
    expect(screen.getByText('2/3')).toBeInTheDocument();
    expect(screen.queryByText('Which store?')).not.toBeInTheDocument();
  });

  it('submits single, multi-select, and Other answers by stable indexes', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire(questionnaireRequest, onSubmit);

    await user.click(screen.getByRole('radio', { name: /Postgres/i }));
    expect(await screen.findByText('Which layers?', {}, { timeout: 1500 })).toBeInTheDocument();

    await user.click(screen.getByRole('checkbox', { name: 'Frontend' }));
    expect(screen.getByRole('checkbox', { name: 'Frontend' })).toBeChecked();
    await user.click(screen.getByRole('checkbox', { name: 'Backend' }));
    fireEvent.change(screen.getByRole('textbox', { name: '其他: Which layers?' }), {
      target: { value: '  Keep mobile unchanged  ' },
    });
    goNext();

    expect(await screen.findByText('Anything else?')).toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox', { name: '其他: Anything else?' }), {
      target: { value: '  请使用中文  ' },
    });
    fireEvent.click(screen.getByRole('button', { name: '提交回答' }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [
            { questionIndex: 0, selectedOptionIndexes: [1] },
            {
              questionIndex: 1,
              selectedOptionIndexes: [0, 1],
              otherText: '  Keep mobile unchanged  ',
            },
            {
              questionIndex: 2,
              selectedOptionIndexes: [],
              otherText: '  请使用中文  ',
            },
          ],
        },
      });
    });
  });

  it('auto-submits when the last question is answered by single-select', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'questionnaire-last-auto',
      input: {
        mode: 'default',
        questions: [
          {
            question: 'Only one?',
            multiSelect: false,
            options: [
              { label: 'Yes' },
              { label: 'No' },
            ],
          },
        ],
      },
    }, onSubmit);

    fireEvent.click(screen.getByRole('radio', { name: 'Yes' }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'accepted',
          answers: [{ questionIndex: 0, selectedOptionIndexes: [0] }],
        },
      });
    });
  });

  it('keeps multi-select independent and allows Other to be cleared without auto-advance', async () => {
    renderQuestionnaire();

    fireEvent.click(screen.getByRole('radio', { name: /SQLite/i }));
    expect(await screen.findByText('Which layers?', {}, { timeout: 1500 })).toBeInTheDocument();

    const multiOther = screen.getByRole('checkbox', { name: /其他/i });
    fireEvent.click(multiOther);
    expect(multiOther).toBeChecked();
    fireEvent.click(multiOther);
    expect(multiOther).not.toBeChecked();
    // Still on multi-select question — no auto advance
    expect(screen.getByText('Which layers?')).toBeInTheDocument();
  });

  it('requires a non-blank answer before submit', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire(questionnaireRequest, onSubmit);

    await waitFor(() => expect(screen.getByRole('radio', { name: /SQLite/i })).toBeInTheDocument());
    // Jump to last question via next buttons without answering
    fireEvent.click(screen.getByRole('button', { name: '下一题' }));
    fireEvent.click(screen.getByRole('button', { name: '下一题' }));
    fireEvent.click(screen.getByRole('button', { name: /提交回答/ }));

    expect(await screen.findByRole('alert')).toHaveTextContent('请回答此问题');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('focuses the Other choice when a question has no predefined options', async () => {
    renderQuestionnaire({
      ...questionnaireRequest,
      requestId: 'questionnaire-freeform',
      input: {
        mode: 'default',
        questions: [{ question: 'What should change?', options: [] }],
      },
    });

    await waitFor(() => {
      expect(screen.getByRole('radio', { name: '其他' })).toHaveFocus();
    });
  });

  it('shows plan-only actions only for a plan questionnaire', async () => {
    const onSubmit = vi.fn(async () => undefined);
    const planRequest: AcpInteractionRequest = {
      ...questionnaireRequest,
      requestId: 'questionnaire-plan',
      input: {
        ...questionnaireRequest.input,
        mode: 'plan',
      },
    };
    renderQuestionnaire(planRequest, onSubmit);

    fireEvent.click(screen.getByRole('button', { name: '下一题' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Frontend' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Backend' }));
    fireEvent.click(screen.getByRole('button', { name: '讨论这些回答' }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: {
          outcome: 'chat_about_this',
          answers: [{ questionIndex: 1, selectedOptionIndexes: [0, 1] }],
        },
      });
    });
  });

  it('hides plan-only actions in default mode and submits cancellation explicitly', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire(questionnaireRequest, onSubmit);

    expect(screen.queryByRole('button', { name: '讨论这些回答' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '跳过提问并开始规划' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '取消' }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        questionnaire: { outcome: 'cancelled', answers: [] },
      });
    });
  });

  it('keeps the questionnaire available after a submission error', async () => {
    const onSubmit = vi.fn()
      .mockRejectedValueOnce(new Error('transport closed'))
      .mockResolvedValueOnce(undefined);
    renderQuestionnaire(questionnaireRequest, onSubmit);

    fireEvent.click(screen.getByRole('radio', { name: /SQLite/i }));
    expect(await screen.findByText('Which layers?', {}, { timeout: 1500 })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '下一题' }));
    fireEvent.click(screen.getByRole('button', { name: '提交回答' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('transport closed');

    await waitFor(() => {
      expect(screen.getByRole('button', { name: '提交回答' })).toBeEnabled();
    });
    fireEvent.click(screen.getByRole('button', { name: '提交回答' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2));
  });
});
