import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAcpStore } from '@/stores/acpStore';
import { useUIStore } from '@/stores';
import { AgentPage } from '../AgentPage';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => ({
      'agentPage.loading': '加载中',
      'agentPage.noAgents': '请先在 设置 → ACP Agent 中启用至少一个 Agent',
      'agentPage.openSettings': '打开 ACP 设置',
    })[key] ?? key,
  }),
}));

vi.mock('@/components/acp/AcpSidebar', () => ({
  AcpSidebar: () => <aside data-testid="acp-sidebar" />,
}));

vi.mock('@/components/acp/AcpConversationPane', () => ({
  AcpConversationPane: () => <main data-testid="acp-conversation-pane" />,
}));

vi.mock('@/lib/invoke', () => ({
  invoke: vi.fn(() => new Promise(() => {})),
  listen: vi.fn(),
}));

describe('AgentPage', () => {
  beforeEach(() => {
    act(() => {
      useAcpStore.setState({
        config: null,
        configReady: true,
        projectsReady: true,
        threadsReady: true,
      });
      useUIStore.setState({
        activePage: 'agent',
        settingsSection: 'general',
      });
    });
  });

  it('shows a full-page setup prompt when no agent is enabled', () => {
    render(<AgentPage />);

    expect(screen.getByTestId('acp-unconfigured-empty-state')).toBeInTheDocument();
    expect(screen.queryByTestId('acp-sidebar')).not.toBeInTheDocument();
    expect(screen.queryByTestId('acp-conversation-pane')).not.toBeInTheDocument();
  });

  it('opens the ACP Agent settings from the setup prompt', () => {
    render(<AgentPage />);

    fireEvent.click(screen.getByRole('button', { name: '打开 ACP 设置' }));

    expect(useUIStore.getState().activePage).toBe('settings');
    expect(useUIStore.getState().settingsSection).toBe('acpAgents');
  });

  it('keeps the two-pane workbench when an agent is enabled', () => {
    act(() => {
      useAcpStore.setState({
        config: {
          general: {
            idleTimeoutSecs: 300,
            maxConcurrentProcesses: 4,
            permissionDefault: 'default',
            registryRefresh: 'daily',
          },
          agents: [{
            id: 'codex',
            name: 'Codex',
            source: 'custom',
            command: 'codex',
            args: [],
            enabled: true,
            sort: 0,
          }],
        },
      });
    });

    render(<AgentPage />);

    expect(screen.getByTestId('acp-sidebar')).toBeInTheDocument();
    expect(screen.getByTestId('acp-conversation-pane')).toBeInTheDocument();
  });
});
