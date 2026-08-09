import { useEffect, useState } from 'react';
import { Alert, Button, Empty, Spin, theme } from 'antd';
import { useTranslation } from 'react-i18next';
import { useAcpStore } from '@/stores/acpStore';
import { useUIStore } from '@/stores';
import { AcpSidebar } from '@/components/acp/AcpSidebar';
import { AcpConversationPane } from '@/components/acp/AcpConversationPane';

/**
 * ACP Agent workbench — layout is a 1:1 copy of ChatPage shell:
 * left 256px sidebar + right conversation pane.
 */
export function AgentPage() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const config = useAcpStore((s) => s.config);
  const configReady = useAcpStore((s) => s.configReady);
  const configError = useAcpStore((s) => s.configError);
  const loadConfig = useAcpStore((s) => s.loadConfig);
  const loadProjects = useAcpStore((s) => s.loadProjects);
  const loadAllThreads = useAcpStore((s) => s.loadAllThreads);
  const restoreLastSession = useAcpStore((s) => s.restoreLastSession);
  const setActivePage = useUIStore((s) => s.setActivePage);
  const setSettingsSection = useUIStore((s) => s.setSettingsSection);
  const [retryingConfig, setRetryingConfig] = useState(false);

  useEffect(() => {
    // Revalidate lists, then re-open the last project conversation.
    // Agent page unmounts on leave (`agent: 'unmount'`), so selection is restored
    // from the persisted store every time the module is entered.
    let cancelled = false;
    void (async () => {
      // Wait for zustand persist rehydration so activeProjectId/activeThreadId
      // from the previous session are available before restore.
      await new Promise<void>((resolve) => {
        const api = useAcpStore.persist;
        if (api.hasHydrated()) {
          resolve();
          return;
        }
        const unsub = api.onFinishHydration(() => {
          unsub();
          resolve();
        });
      });
      if (cancelled) return;
      await Promise.all([loadConfig(), loadProjects(), loadAllThreads()]);
      if (cancelled) return;
      await restoreLastSession();
    })();
    return () => {
      cancelled = true;
    };
  }, [loadConfig, loadProjects, loadAllThreads, restoreLastSession]);

  const openAcpSettings = () => {
    setSettingsSection('acpAgents');
    setActivePage('settings');
  };

  const retryConfig = async () => {
    setRetryingConfig(true);
    try {
      await loadConfig();
    } finally {
      setRetryingConfig(false);
    }
  };

  if (!configReady) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin tip={t('agentPage.loading')} />
      </div>
    );
  }

  if (configError && !config) {
    return (
      <div
        className="flex h-full items-center justify-center"
        data-testid="acp-config-error-state"
        style={{ backgroundColor: token.colorBgElevated }}
      >
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={(
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              <span>{t('error.loadFailed')}</span>
              <span style={{ color: token.colorTextSecondary, overflowWrap: 'anywhere' }}>
                {configError}
              </span>
            </div>
          )}
        >
          <Button type="primary" loading={retryingConfig} onClick={() => void retryConfig()}>
            {t('agentPage.retryConnection')}
          </Button>
        </Empty>
      </div>
    );
  }

  const hasEnabledAgent = config?.agents.some((agent) => agent.enabled) ?? false;
  const configWarning = configError ? (
    <Alert
      type="warning"
      showIcon
      title={t('error.loadFailed')}
      description={configError}
      action={(
        <Button size="small" loading={retryingConfig} onClick={() => void retryConfig()}>
          {t('agentPage.retryConnection')}
        </Button>
      )}
      style={{ flexShrink: 0, margin: 8 }}
    />
  ) : null;

  if (!hasEnabledAgent) {
    return (
      <div
        className="flex h-full flex-col"
        style={{ backgroundColor: token.colorBgElevated }}
      >
        {configWarning}
        <div
          className="flex min-h-0 flex-1 items-center justify-center"
          data-testid="acp-unconfigured-empty-state"
        >
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t('agentPage.noAgents')}
          >
            <Button type="primary" onClick={openAcpSettings}>
              {t('agentPage.openSettings')}
            </Button>
          </Empty>
        </div>
      </div>
    );
  }

  return (
    <div
      className="flex h-full flex-col"
      style={{ overflow: 'hidden', contain: 'layout paint style' }}
    >
      {configWarning}
      <div className="flex min-h-0 flex-1">
        <div
          className="h-full shrink-0"
          data-testid="acp-sidebar-shell"
          style={{
            width: 256,
            borderRight: '1px solid var(--border-color)',
            backgroundColor: token.colorBgContainer,
            overflow: 'hidden',
            contain: 'layout paint',
          }}
        >
          <div
            data-testid="acp-sidebar-content"
            style={{
              width: 256,
              height: '100%',
            }}
          >
            <AcpSidebar />
          </div>
        </div>
        <div
          style={{
            flex: 1,
            minWidth: 0,
            display: 'flex',
            flexDirection: 'column',
            overflow: 'hidden',
            backgroundColor: token.colorBgElevated,
          }}
        >
          <AcpConversationPane />
        </div>
      </div>
    </div>
  );
}
