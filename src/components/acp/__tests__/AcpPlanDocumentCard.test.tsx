import { App } from 'antd';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { translateZhCN } from '@/test/i18nTestTranslator';
import type { AcpPlanDocument } from '@/stores/acpStore';
import { AcpPlanDocumentCard } from '../AcpPlanDocumentCard';

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => ({
    t: translateZhCN,
  }),
}));

const planDocument: AcpPlanDocument = {
  id: 'plan-1',
  threadId: 'thread-1',
  content: '',
  title: 'Release plan',
  status: 'approved',
  sequence: 1,
  createdAt: '2026-08-09T00:00:00.000Z',
};

describe('AcpPlanDocumentCard', () => {
  it('opens an accessible dialog and restores focus after Escape', async () => {
    const user = userEvent.setup();
    render(
      <App>
        <AcpPlanDocumentCard document={planDocument} />
      </App>,
    );

    await user.click(screen.getByRole('button', { name: '全屏查看' }));
    const dialog = screen.getByRole('dialog', { name: 'Release plan' });
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveAttribute('aria-modal', 'true');

    await user.keyboard('{Escape}');

    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Release plan' })).not.toBeInTheDocument();
    });
    await waitFor(() => {
      expect(screen.getByRole('button', { name: '全屏查看' })).toHaveFocus();
    });
  });
});
