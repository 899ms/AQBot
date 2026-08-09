import { useEffect } from 'react';
import { Button, Empty, Spin, theme } from 'antd';
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
  const loadConfig = useAcpStore((s) => s.loadConfig);
  const loadProjects = useAcpStore((s) => s.loadProjects);
  const loadAllThreads = useAcpStore((s) => s.loadAllThreads);
  const restoreLastSession = useAcpStore((s) => s.restoreLastSession);
  const setActivePage = useUIStore((s) => s.setActivePage);
  const setSettingsSection = useUIStore((s) => s.setSettingsSection);

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

  if (!configReady) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin tip={t('agentPage.loading')} />
      </div>
    );
  }

  const hasEnabledAgent = config?.agents.some((agent) => agent.enabled) ?? false;

  if (!hasEnabledAgent) {
    return (
      <div
        className="flex h-full items-center justify-center"
        data-testid="acp-unconfigured-empty-state"
        style={{ backgroundColor: token.colorBgElevated }}
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
    );
  }

  return (
    <div
      className="flex h-full"
      style={{ overflow: 'hidden', contain: 'layout paint style' }}
    >
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
  );
}
