import { App } from 'antd';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CherryStudioImportModal } from '../CherryStudioImportModal';

const { invokeMock, openMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  openMock: vi.fn(),
}));

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openMock,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string | Record<string, unknown>) =>
      typeof fallback === 'string' ? fallback : key,
  }),
}));

describe('CherryStudioImportModal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    openMock.mockResolvedValue('/Users/test/cherry.zip');
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'scan_cherry_studio_import') {
        return {
          conversationCount: 2,
          messageCount: 12,
          fileCount: 1,
          importableProviderCount: 1,
          importableRoleCount: 3,
          skippedEmptyTopicCount: 1,
          skippedEmptyAssistantCount: 1,
          duplicateConversationCount: 0,
          duplicateRoleCount: 0,
          warnings: [{ code: 'unsupported_block_type', message: 'Tool block preserved', sourceId: 'b1' }],
        };
      }
      if (command === 'import_cherry_studio_backup') {
        return {
          importedConversationCount: 2,
          importedMessageCount: 12,
          importedFileCount: 1,
          importedProviderCount: 0,
          skippedDuplicateConversationCount: 0,
          warnings: [],
        };
      }
      return undefined;
    });
  });

  it('scans a selected Cherry Studio backup and previews counts', async () => {
    const user = userEvent.setup();

    render(
      <App>
        <CherryStudioImportModal open onClose={vi.fn()} onImported={vi.fn()} />
      </App>,
    );

    expect(screen.getByText('settings.cherryImport.supportedFormats')).toBeInTheDocument();
    expect(screen.getByText('settings.cherryImport.exportPath')).toBeInTheDocument();

    await user.click(screen.getByText('settings.cherryImport.uploadHint'));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('scan_cherry_studio_import', {
        path: '/Users/test/cherry.zip',
      });
    });
    expect(await screen.findByText('settings.cherryImport.preview')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('12')).toBeInTheDocument();
    // Mock t() returns the key; helper falls back to backend message when key is untranslated.
    expect(screen.getByText('Tool block preserved')).toBeInTheDocument();
  });

  it('imports without provider keys by default and can opt in', async () => {
    const user = userEvent.setup();
    const onImported = vi.fn();

    render(
      <App>
        <CherryStudioImportModal open onClose={vi.fn()} onImported={onImported} />
      </App>,
    );

    await user.click(screen.getByText('settings.cherryImport.uploadHint'));
    await screen.findByText('settings.cherryImport.preview');
    await user.click(screen.getByLabelText('settings.cherryImport.importProviderKeys'));
    await user.click(screen.getByRole('button', { name: 'common.confirm' }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('import_cherry_studio_backup', {
        path: '/Users/test/cherry.zip',
        options: { importProviderKeys: true },
      });
      expect(onImported).toHaveBeenCalledTimes(1);
    });
  });
});
