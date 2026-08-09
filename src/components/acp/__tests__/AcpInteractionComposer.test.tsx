import { App } from 'antd';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { translateZhCN } from '@/test/i18nTestTranslator';
import {
  AcpInteractionComposer,
  type AcpInteractionSubmission,
  type AcpInteractionRequest,
} from '../AcpInteractionComposer';

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => ({
    t: translateZhCN,
  }),
}));

const baseRequest: AcpInteractionRequest = {
  threadId: 'thread-1',
  messageId: 'assistant-1',
  requestId: 'permission-1',
  toolName: 'run_terminal_command',
  input: { command: 'pnpm test' },
  status: 'pending',
  options: [
    {
      id: 'allow-once',
      label: 'Allow once from Agent',
      kind: 'AllowOnce',
      description: 'Only run this command once.',
      variant: 'primary',
    },
    {
      id: 'reject-once',
      label: 'Reject from Agent',
      kind: 'RejectOnce',
      variant: 'danger',
    },
  ],
};

function renderComposer(
  request: AcpInteractionRequest = baseRequest,
  onSubmit: (submission: AcpInteractionSubmission) => Promise<void> = vi.fn(async () => undefined),
) {
  render(
    <App>
      <AcpInteractionComposer request={request} onSubmit={onSubmit} />
    </App>,
  );
}

describe('AcpInteractionComposer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders a pending permission as a localized, accessible decision form', () => {
    renderComposer();

    expect(screen.getByRole('group', { name: '需要权限' })).toHaveAttribute(
      'aria-live',
      'polite',
    );
    expect(screen.getByRole('group', { name: '需要权限' })).toBeInTheDocument();
    expect(screen.getByText('run_terminal_command')).toHaveAttribute('translate', 'no');
    expect(screen.getByRole('button', { name: '允许一次' })).toHaveAccessibleDescription(
      'Only run this command once.',
    );
    // Agents often omit allow_always — UI injects session-scoped always allow.
    expect(screen.getByRole('button', { name: '始终允许' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /拒绝/i })).toBeInTheDocument();
    expect(screen.queryByText('Allow once from Agent')).not.toBeInTheDocument();
    expect(screen.getByText('Only run this command once.')).toHaveStyle({
      overflowWrap: 'anywhere',
    });

    const details = screen.getByText('请求详情').closest('details');
    expect(details).toBeInTheDocument();
    expect(details).toHaveTextContent('pnpm test');
  });

  it('submits the synthetic always-allow option id for session scope', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderComposer(baseRequest, onSubmit);

    fireEvent.click(screen.getByRole('button', { name: '始终允许' }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        optionId: '__aqbot_session_always_allow',
      });
    });
  });

  it('moves keyboard focus to the first available decision', async () => {
    renderComposer();

    await waitFor(() => {
      expect(screen.getByRole('button', { name: '允许一次' })).toHaveFocus();
    });
  });

  it.each([
    ['question', '需要你的回答'],
    ['plan_review', '审核计划'],
  ] as const)('localizes the %s interaction title', (kind, title) => {
    renderComposer({ ...baseRequest, kind });

    expect(screen.getByRole('form', { name: title })).toBeInTheDocument();
  });

  it('keeps arbitrary question choices and their descriptions intact', () => {
    renderComposer({
      ...baseRequest,
      kind: 'question',
      question: 'Which database should this project use?',
      options: [
        {
          id: 'answer:0',
          label: 'SQLite',
          description: 'A local file with no server dependency.',
        },
      ],
    });

    expect(screen.getByText('Which database should this project use?')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /SQLite/i })).toHaveAttribute('translate', 'no');
    expect(screen.getByText('A local file with no server dependency.')).toBeInTheDocument();
  });

  it('localizes the known plan review actions', () => {
    renderComposer({
      ...baseRequest,
      kind: 'plan_review',
      title: 'Plan review',
      description: '## Plan\n1. Inspect\n2. Ship',
      options: [
        { id: 'approved', label: 'Approve and implement' },
        { id: 'cancelled', label: 'Continue planning' },
        { id: 'abandoned', label: 'Abandon plan' },
      ],
    });

    expect(screen.getByRole('button', { name: '立即执行' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '进行改变' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '取消' })).toBeInTheDocument();
  });

  it('lets plan review request changes with feedback', async () => {
    const onSubmit = vi.fn(async () => undefined);
    renderComposer({
      ...baseRequest,
      kind: 'plan_review',
      description: 'Ship the rename plan.',
      options: [
        { id: 'approved', label: 'Approve and implement' },
        { id: 'cancelled', label: 'Continue planning' },
        { id: 'abandoned', label: 'Abandon plan' },
      ],
    }, onSubmit);

    fireEvent.click(screen.getByRole('button', { name: '进行改变' }));
    fireEvent.change(
      screen.getByPlaceholderText('描述希望如何调整计划…'),
      { target: { value: '  Keep the data path unchanged  ' } },
    );
    fireEvent.click(screen.getByRole('button', { name: '提交修改意见' }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        optionId: 'cancelled',
        feedback: 'Keep the data path unchanged',
      });
    });
  });

  it('expands plan review content to a near-fullscreen overlay', () => {
    renderComposer({
      ...baseRequest,
      kind: 'plan_review',
      description: '## Plan\nDetails for review',
      options: [
        { id: 'approved', label: 'Approve and implement' },
        { id: 'cancelled', label: 'Continue planning' },
        { id: 'abandoned', label: 'Abandon plan' },
      ],
    });

    const expand = screen.getByRole('button', { name: '全屏查看' });
    fireEvent.click(expand);
    expect(screen.getByRole('button', { name: '退出全屏' })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: '退出全屏' }));
    expect(screen.getByRole('button', { name: '全屏查看' })).toHaveAttribute('aria-pressed', 'false');
  });

  it('disables every choice while submitting and reports the selected option', async () => {
    let resolveSubmit!: () => void;
    const onSubmit = vi.fn(() => new Promise<void>((resolve) => {
      resolveSubmit = resolve;
    }));
    renderComposer(baseRequest, onSubmit);

    fireEvent.click(screen.getByRole('button', { name: /允许一次/i }));

    expect(onSubmit).toHaveBeenCalledWith({ optionId: 'allow-once' });
    expect(screen.getByRole('form', { name: '需要权限' })).toHaveAttribute(
      'aria-busy',
      'true',
    );
    expect(screen.getByRole('button', { name: /允许一次/i })).toBeDisabled();
    expect(screen.getByRole('button', { name: /拒绝/i })).toBeDisabled();

    resolveSubmit();
    await waitFor(() => {
      expect(screen.getByRole('form', { name: '需要权限' })).toHaveAttribute(
        'aria-busy',
        'false',
      );
    });
  });

  it('shows a retryable inline error when submission fails', async () => {
    const onSubmit = vi.fn()
      .mockRejectedValueOnce(new Error('connection lost'))
      .mockResolvedValueOnce(undefined);
    renderComposer(baseRequest, onSubmit);

    fireEvent.click(screen.getByRole('button', { name: /允许一次/i }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('提交失败，请重试');
    expect(alert).toHaveTextContent('connection lost');
    expect(screen.getByRole('button', { name: /允许一次/i })).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: /允许一次/i }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2));
  });

  it.each(['approved', 'denied'] as const)('does not render a %s interaction', (status) => {
    render(
      <App>
        <AcpInteractionComposer
          request={{ ...baseRequest, status }}
          onSubmit={vi.fn(async () => undefined)}
        />
      </App>,
    );

    expect(screen.queryByRole('form')).not.toBeInTheDocument();
  });
});
