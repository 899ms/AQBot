import { App } from 'antd';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  AcpInteractionComposer,
  type AcpInteractionRequest,
  type AcpInteractionSubmission,
} from '../AcpInteractionComposer';

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => ({
    t: (_key: string, fallback?: string | { defaultValue?: string }) =>
      typeof fallback === 'string' ? fallback : (fallback?.defaultValue ?? _key),
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

  it('auto-advances after a single-select choice', async () => {
    renderQuestionnaire();

    fireEvent.click(screen.getByRole('radio', { name: /Postgres/i }));
    expect(await screen.findByText('Which layers?', {}, { timeout: 1500 })).toBeInTheDocument();
    expect(screen.getByText('2/3')).toBeInTheDocument();
    expect(screen.queryByText('Which store?')).not.toBeInTheDocument();
  });

  it('submits single, multi-select, and Other answers by stable indexes', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderQuestionnaire(questionnaireRequest, onSubmit);

    fireEvent.click(screen.getByRole('radio', { name: /Postgres/i }));
    expect(await screen.findByText('Which layers?', {}, { timeout: 1500 })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('checkbox', { name: 'Frontend' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Backend' }));
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

    expect(await screen.findByRole('alert')).toHaveTextContent('请至少回答一个问题');
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
