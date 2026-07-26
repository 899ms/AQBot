import { App, ConfigProvider } from 'antd';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { PreviousCrashReport } from '@/types';
import { CrashRecoveryModal } from '../CrashRecoveryModal';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  revealItemInDir: vi.fn(),
  writeText: vi.fn(),
  t: (key: string) => key,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: mocks.t,
  }),
}));

vi.mock('@/lib/invoke', () => ({
  isTauri: () => true,
  invoke: mocks.invoke,
}));

vi.mock('@tauri-apps/plugin-opener', () => ({
  revealItemInDir: mocks.revealItemInDir,
}));

const report: PreviousCrashReport = {
  id: 'crash-1',
  crashed_at: '2026-07-26T01:44:11Z',
  app_version: '0.0.103',
  bundle_id: 'top.aqbot.desktop.dev',
  signal: 'SIGTRAP',
  reason: 'Trace/BPT trap: 5',
  summary: 'Must only be used from the main thread\n-[NSPanel setFloatingPanel:]',
  log_path: '/Users/test/.aqbot/logs/aqbot.log',
  system_report_path: '/Users/test/Library/Logs/DiagnosticReports/AQBot.ips',
};

function renderModal() {
  return render(
    <ConfigProvider theme={{ token: { motion: false } }}>
      <App>
        <CrashRecoveryModal />
      </App>
    </ConfigProvider>,
  );
}

describe('CrashRecoveryModal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: mocks.writeText },
    });
    mocks.writeText.mockResolvedValue(undefined);
    mocks.revealItemInDir.mockResolvedValue(undefined);
  });

  it('stays hidden when no previous crash is pending', async () => {
    mocks.invoke.mockResolvedValueOnce(null);
    renderModal();

    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith('get_previous_crash_report');
    });
    expect(screen.queryByText('crashRecovery.title')).not.toBeInTheDocument();
  });

  it('shows crash details and diagnostic file actions', async () => {
    mocks.invoke.mockResolvedValueOnce(report);
    const user = userEvent.setup();
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: mocks.writeText },
    });
    renderModal();

    expect(await screen.findByText('crashRecovery.title')).toBeInTheDocument();
    expect(screen.getByText('SIGTRAP')).toBeInTheDocument();
    expect(screen.getByText(/Must only be used from the main thread/)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'crashRecovery.copy' }));
    expect(mocks.writeText).toHaveBeenCalledWith(expect.stringContaining('SIGTRAP'));

    await user.click(screen.getByRole('button', { name: 'crashRecovery.revealLog' }));
    expect(mocks.revealItemInDir).toHaveBeenCalledWith(report.log_path);

    await user.click(screen.getByRole('button', { name: 'crashRecovery.revealReport' }));
    expect(mocks.revealItemInDir).toHaveBeenCalledWith(report.system_report_path);
  });

  it('acknowledges the exact report before closing', async () => {
    mocks.invoke.mockResolvedValueOnce(report).mockResolvedValueOnce(undefined);
    const user = userEvent.setup();
    renderModal();

    await user.click(await screen.findByRole('button', {
      name: 'crashRecovery.acknowledge',
    }));

    expect(mocks.invoke).toHaveBeenLastCalledWith(
      'acknowledge_previous_crash_report',
      { id: report.id },
    );
    await waitFor(() => {
      expect(screen.queryByText('crashRecovery.title')).not.toBeInTheDocument();
    });
  });

  it('reports load failures instead of silently hiding them', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    mocks.invoke.mockRejectedValueOnce(new Error('diagnostics unavailable'));
    renderModal();

    expect(await screen.findByText('crashRecovery.loadFailed')).toBeInTheDocument();
    expect(consoleError).toHaveBeenCalledWith(
      'Could not load previous AQBot crash report:',
      expect.any(Error),
    );
    consoleError.mockRestore();
  });

  it('keeps the report visible when acknowledgement fails', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    mocks.invoke
      .mockResolvedValueOnce(report)
      .mockRejectedValueOnce(new Error('write failed'));
    const user = userEvent.setup();
    renderModal();

    await user.click(await screen.findByRole('button', {
      name: 'crashRecovery.acknowledge',
    }));

    expect(await screen.findByText('crashRecovery.acknowledgeFailed')).toBeInTheDocument();
    expect(screen.getByText('crashRecovery.title')).toBeInTheDocument();
    consoleError.mockRestore();
  });
});
