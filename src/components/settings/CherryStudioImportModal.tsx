import { useState } from 'react';
import { Alert, App, Checkbox, Divider, Modal, Space, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { invoke } from '@/lib/invoke';
import { getErrorMessage } from '@/lib/errorMessage';
import { getThirdPartyImportWarningMessage } from '@/lib/thirdPartyImportWarnings';
import type { CherryStudioImportResult, CherryStudioImportSummary } from '@/types';
import { ThirdPartyImportUpload } from './ThirdPartyImportUpload';

const { Text, Title } = Typography;

type Props = {
  open: boolean;
  onClose: () => void;
  onImported: (result: CherryStudioImportResult) => void;
};

function CountItem({ label, value }: { label: string; value: number }) {
  return (
    <div style={{ minWidth: 96 }}>
      <Text type="secondary" style={{ fontSize: 12 }}>{label}</Text>
      <div style={{ fontSize: 20, fontWeight: 600, lineHeight: 1.2 }}>{value}</div>
    </div>
  );
}

export function CherryStudioImportModal({ open, onClose, onImported }: Props) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [path, setPath] = useState<string | null>(null);
  const [summary, setSummary] = useState<CherryStudioImportSummary | null>(null);
  const [scanLoading, setScanLoading] = useState(false);
  const [importLoading, setImportLoading] = useState(false);
  const [importProviderKeys, setImportProviderKeys] = useState(false);

  const reset = () => {
    setPath(null);
    setSummary(null);
    setImportProviderKeys(false);
  };

  const handleClose = () => {
    reset();
    onClose();
  };

  const handleSelectedPath = async (selected: string) => {
    try {
      setPath(selected);
      setSummary(null);
      setScanLoading(true);
      const nextSummary = await invoke<CherryStudioImportSummary>('scan_cherry_studio_import', {
        path: selected,
      });
      setSummary(nextSummary);
    } catch (error) {
      message.error(getErrorMessage(error));
    } finally {
      setScanLoading(false);
    }
  };

  const handleSelectFile = async () => {
    try {
      const { open: openFile } = await import('@tauri-apps/plugin-dialog');
      const selected = await openFile({
        multiple: false,
        filters: [{ name: 'Cherry Studio', extensions: ['zip', 'json'] }],
      });
      if (!selected || typeof selected !== 'string') return;

      await handleSelectedPath(selected);
    } catch (error) {
      message.error(getErrorMessage(error));
    }
  };

  const handleImport = async () => {
    if (!path || !summary) return;
    setImportLoading(true);
    try {
      const result = await invoke<CherryStudioImportResult>('import_cherry_studio_backup', {
        path,
        options: { importProviderKeys },
      });
      message.success(t('settings.cherryImport.success'));
      onImported(result);
      reset();
      onClose();
    } catch (error) {
      message.error(getErrorMessage(error));
    } finally {
      setImportLoading(false);
    }
  };

  return (
    <Modal
      open={open}
      title={t('settings.cherryImport.title')}
      onCancel={handleClose}
      onOk={handleImport}
      okText={t('common.confirm')}
      cancelText={t('common.cancel')}
      okButtonProps={{ disabled: !summary }}
      confirmLoading={importLoading}
      width={640}
    >
      <Space orientation="vertical" size={14} style={{ width: '100%' }}>
        <ThirdPartyImportUpload
          accept=".zip,.json"
          active={open}
          exportPath={t('settings.cherryImport.exportPath')}
          loading={scanLoading}
          path={path}
          supportedFormats={t('settings.cherryImport.supportedFormats')}
          uploadHint={t('settings.cherryImport.uploadHint')}
          onPathSelected={handleSelectedPath}
          onPickFile={handleSelectFile}
        />

        {summary && (
          <>
            <Divider style={{ margin: '2px 0' }} />
            <Title level={5} style={{ margin: 0 }}>{t('settings.cherryImport.preview')}</Title>
            <Space wrap size={18}>
              <CountItem label={t('settings.cherryImport.conversations')} value={summary.conversationCount} />
              <CountItem label={t('settings.cherryImport.messages')} value={summary.messageCount} />
              <CountItem label={t('settings.cherryImport.files')} value={summary.fileCount} />
              <CountItem label={t('settings.cherryImport.roles')} value={summary.importableRoleCount ?? 0} />
              <CountItem label={t('settings.cherryImport.providers')} value={summary.importableProviderCount} />
              <CountItem label={t('settings.cherryImport.duplicates')} value={summary.duplicateConversationCount} />
            </Space>
            <Checkbox
              checked={importProviderKeys}
              disabled={summary.importableProviderCount === 0}
              onChange={(event) => setImportProviderKeys(event.target.checked)}
            >
              {t('settings.cherryImport.importProviderKeys')}
            </Checkbox>
            {summary.skippedEmptyTopicCount > 0 && (
              <Alert
                type="info"
                showIcon
                message={t('settings.cherryImport.emptyTopics', { count: summary.skippedEmptyTopicCount })}
              />
            )}
            {(summary.skippedEmptyAssistantCount ?? 0) > 0 && (
              <Alert
                type="info"
                showIcon
                message={t('settings.cherryImport.emptyAssistants', {
                  count: summary.skippedEmptyAssistantCount,
                })}
              />
            )}
            {summary.warnings.length > 0 && (
              <Space orientation="vertical" size={6} style={{ width: '100%' }}>
                {summary.warnings.map((warning, index) => (
                  <Alert
                    key={`${warning.code}-${warning.sourceId ?? index}`}
                    type="warning"
                    showIcon
                    message={getThirdPartyImportWarningMessage(warning, t, 'cherryImport')}
                  />
                ))}
              </Space>
            )}
          </>
        )}
      </Space>
    </Modal>
  );
}
