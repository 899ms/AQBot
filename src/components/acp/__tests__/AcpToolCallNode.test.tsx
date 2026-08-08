import type { ComponentProps } from 'react';
import { App } from 'antd';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAcpStore } from '@/stores/acpStore';
import { AcpToolCallNode } from '../AcpToolCallNode';

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => ({
    t: (key: string, fallback?: string | { defaultValue?: string }) =>
      typeof fallback === 'string' ? fallback : (fallback?.defaultValue ?? key),
  }),
}));

describe('AcpToolCallNode', () => {
  beforeEach(() => {
    useAcpStore.setState({
      activeThreadId: 'thread-1',
      toolCalls: {
        'thread-1:assistant-1:tool-7': {
          threadId: 'thread-1',
          messageId: 'assistant-1',
          toolCallId: 'tool-7',
          toolName: 'terminal',
          status: 'success',
          input: '{"command":"ls"}',
          output: 'README.md',
          approvalStatus: 'approved',
          approvalOptionId: 'allow_once',
          approvalLabel: 'Allow once',
        },
        'thread-1:assistant-2:tool-7': {
          threadId: 'thread-1',
          messageId: 'assistant-2',
          toolCallId: 'tool-7',
          toolName: 'terminal',
          status: 'success',
          output: '/workspace',
        },
      },
    });
  });

  it('keeps the approval and execution result inside the chronological tool row', () => {
    const props = {
      node: {
        type: 'tool-call',
        content: 'ls',
        attrs: { id: 'tool-7', message: 'assistant-1', name: 'terminal' },
      },
    } as unknown as ComponentProps<typeof AcpToolCallNode>;

    render(
      <App>
        <AcpToolCallNode {...props} />
      </App>,
    );

    const trigger = screen.getByRole('button', { name: /terminal.*执行成功.*已批准/i });
    fireEvent.click(trigger);
    expect(screen.getByText('README.md')).toBeInTheDocument();
    expect(screen.queryByText('/workspace')).not.toBeInTheDocument();
    expect(screen.getByText('已批准')).toBeInTheDocument();
  });

  it('localizes a semantic questionnaire action stored as the tool result', () => {
    useAcpStore.setState({
      toolCalls: {
        'thread-1:assistant-3:tool-8': {
          threadId: 'thread-1',
          messageId: 'assistant-3',
          toolCallId: 'tool-8',
          toolName: 'ask_user_question',
          status: 'success',
          output: 'aqbot:questionnaire:skip_interview',
        },
      },
    });
    const props = {
      node: {
        type: 'tool-call',
        content: 'plan interview',
        attrs: { id: 'tool-8', message: 'assistant-3', name: 'ask_user_question' },
      },
    } as unknown as ComponentProps<typeof AcpToolCallNode>;

    render(
      <App>
        <AcpToolCallNode {...props} />
      </App>,
    );

    fireEvent.click(screen.getByRole('button', { name: /ask_user_question.*执行成功/i }));
    expect(screen.getByText('跳过问卷并立即规划')).toBeInTheDocument();
    expect(screen.queryByText('aqbot:questionnaire:skip_interview')).not.toBeInTheDocument();
  });
});
