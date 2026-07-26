import { useCallback, useEffect, useState } from 'react';
import { App, Button, Descriptions, Modal, Space, Typography } from 'antd';
import { Copy, FolderOpen } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { invoke, isTauri } from '@/lib/invoke';
import type { PreviousCrashReport } from '@/types';

const { Paragraph, Text } = Typography;

export function formatCrashReport(report: PreviousCrashReport): string {
  return [
    `AQBot ${report.app_version} (${report.bundle_id})`,
    `Crash time: ${report.crashed_at}`,
    `Signal: ${report.signal ?? 'unknown'}`,
    `Reason: ${report.reason}`,
    `Log: ${report.log_path}`,
    `System report: ${report.system_report_path ?? 'unavailable'}`,
    '',
    report.summary,
  ].join('\n');
}

export function CrashRecoveryModal() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [report, setReport] = useState<PreviousCrashReport | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void invoke<PreviousCrashReport | null>('get_previous_crash_report')
      .then(setReport)
      .catch((error) => {
        console.error('Could not load previous AQBot crash report:', error);
        message.error(t('crashRecovery.loadFailed', { error: String(error) }));
      });
  }, [message, t]);

  const reveal = useCallback(async (path: string) => {
    try {
      const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
      await revealItemInDir(path);
    } catch (error) {
      console.error('Could not reveal AQBot diagnostic file:', error);
      message.error(t('crashRecovery.revealFailed', { error: String(error) }));
    }
  }, [message, t]);

  const copy = useCallback(async () => {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(formatCrashReport(report));
      message.success(t('crashRecovery.copySuccess'));
    } catch (error) {
      console.error('Could not copy AQBot crash report:', error);
      message.error(t('crashRecovery.copyFailed', { error: String(error) }));
    }
  }, [message, report, t]);

  const acknowledge = useCallback(async () => {
    if (!report) return;
    try {
      await invoke('acknowledge_previous_crash_report', { id: report.id });
      setReport(null);
    } catch (error) {
      console.error('Could not acknowledge AQBot crash report:', error);
      message.error(t('crashRecovery.acknowledgeFailed', { error: String(error) }));
    }
  }, [message, report, t]);

  return (
    <Modal
      open={report !== null}
      title={t('crashRecovery.title')}
      width={680}
      centered
      destroyOnHidden
      onCancel={() => void acknowledge()}
      footer={report ? (
        <Space wrap>
          <Button icon={<Copy size={15} />} onClick={() => void copy()}>
            {t('crashRecovery.copy')}
          </Button>
          <Button
            icon={<FolderOpen size={15} />}
            onClick={() => void reveal(report.log_path)}
          >
            {t('crashRecovery.revealLog')}
          </Button>
          {report.system_report_path && (
            <Button
              icon={<FolderOpen size={15} />}
              onClick={() => void reveal(report.system_report_path!)}
            >
              {t('crashRecovery.revealReport')}
            </Button>
          )}
          <Button type="primary" onClick={() => void acknowledge()}>
            {t('crashRecovery.acknowledge')}
          </Button>
        </Space>
      ) : null}
    >
      {report && (
        <Space orientation="vertical" size={16} style={{ width: '100%' }}>
          <Paragraph>{t('crashRecovery.description')}</Paragraph>
          <Descriptions bordered size="small" column={1}>
            <Descriptions.Item label={t('crashRecovery.crashedAt')}>
              {new Date(report.crashed_at).toLocaleString()}
            </Descriptions.Item>
            <Descriptions.Item label={t('crashRecovery.appVersion')}>
              {report.app_version}
            </Descriptions.Item>
            <Descriptions.Item label={t('crashRecovery.signal')}>
              {report.signal ?? '—'}
            </Descriptions.Item>
            <Descriptions.Item label={t('crashRecovery.reason')}>
              {report.reason}
            </Descriptions.Item>
            <Descriptions.Item label={t('crashRecovery.logPath')}>
              <Text copyable>{report.log_path}</Text>
            </Descriptions.Item>
            <Descriptions.Item label={t('crashRecovery.systemReportPath')}>
              {report.system_report_path
                ? <Text copyable>{report.system_report_path}</Text>
                : t('crashRecovery.noSystemReport')}
            </Descriptions.Item>
          </Descriptions>
          <div>
            <Text strong>{t('crashRecovery.summary')}</Text>
            <pre
              style={{
                marginBlock: 8,
                maxHeight: 220,
                overflow: 'auto',
                whiteSpace: 'pre-wrap',
                userSelect: 'text',
              }}
            >
              {report.summary}
            </pre>
          </div>
        </Space>
      )}
    </Modal>
  );
}
