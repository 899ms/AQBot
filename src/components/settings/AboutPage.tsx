import { Button, Divider, Typography, InputNumber, Switch } from 'antd';
import { Github, Globe, RefreshCw, Terminal } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useState, useEffect, useCallback } from 'react';
import { isTauri } from '@/lib/invoke';
import logoUrl from '@/assets/image/logo.png';
import { useSettingsStore } from '@/stores';
import { useUpdateChecker } from '@/hooks/useUpdateChecker';
import {
  loadDevtoolsContextMenuEnabled,
  openDevtoolsWithDiagnostics,
} from '@/lib/desktopCapabilities';
import { SettingsGroup } from './SettingsGroup';

const { Text } = Typography;

export function AboutPage() {
  const { t } = useTranslation();
  const [checking, setChecking] = useState(false);
  const [appVersion, setAppVersion] = useState('...');
  const [devtoolsEnabled, setDevtoolsEnabled] = useState(import.meta.env.DEV);
  const { checkForUpdate } = useUpdateChecker();
  const autoCheckUpdate = useSettingsStore((s) => s.settings.auto_check_update ?? true);
  const updateCheckInterval = useSettingsStore((s) => s.settings.update_check_interval ?? 60);
  const saveSettings = useSettingsStore((s) => s.saveSettings);

  useEffect(() => {
    if (isTauri()) {
      import('@tauri-apps/api/app').then(({ getVersion }) => {
        getVersion().then(v => setAppVersion(v));
      });
      void loadDevtoolsContextMenuEnabled(import.meta.env.DEV).then(setDevtoolsEnabled);
    }
  }, []);

  const handleCheckUpdate = useCallback(async () => {
    setChecking(true);
    try {
      await checkForUpdate();
    } finally {
      setChecking(false);
    }
  }, [checkForUpdate]);

  const rowStyle = { padding: '4px 0' };

  const handleOpenDevTools = useCallback(async () => {
    if (isTauri()) {
      await openDevtoolsWithDiagnostics();
    }
  }, []);

  return (
    <div className="p-6 pb-12">
      {/* Logo + App Name (macOS-style) */}
      <div style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        padding: '32px 0 24px',
      }}>
        <img
          src={logoUrl}
          alt="AQBot"
          style={{ width: 96, height: 96, borderRadius: 20, marginBottom: 16 }}
          draggable={false}
        />
        <div style={{ fontSize: 22, fontWeight: 600 }}>AQBot</div>
        <Text type="secondary" style={{ marginTop: 4 }}>
          {t('settings.version')} {appVersion}
        </Text>
      </div>

      <SettingsGroup title={t('settings.groupAppInfo')}>
        <div style={rowStyle} className="flex items-center justify-between">
          <span>{t('settings.version')}</span>
          <Text type="secondary">{appVersion}</Text>
        </div>
        <Divider style={{ margin: '4px 0' }} />
        <div style={rowStyle} className="flex items-center justify-between">
          <span>{t('settings.openSource')}</span>
          <Text type="secondary">AGPL-3.0</Text>
        </div>
      </SettingsGroup>
      <SettingsGroup title={t('settings.groupLinks')}>
        <div style={rowStyle} className="flex items-center justify-between">
          <span>{t('settings.website')}</span>
          <Button
            icon={<Globe size={16} />}
            href="https://app.aqbot.top"
            target="_blank"
            type="link"
          >
            {t('settings.website')}
          </Button>
        </div>
        <Divider style={{ margin: '4px 0' }} />
        <div style={rowStyle} className="flex items-center justify-between">
          <span>GitHub</span>
          <Button
            icon={<Github size={16} />}
            href="https://github.com/AQBot-Desktop/AQBot"
            target="_blank"
            type="link"
          >
            {t('settings.github')}
          </Button>
        </div>
        <Divider style={{ margin: '4px 0' }} />
        <div style={rowStyle} className="flex items-center justify-between">
          <span>{t('settings.checkUpdate')}</span>
          <Button
            icon={<RefreshCw size={16} className={checking ? 'animate-spin' : ''} />}
            onClick={handleCheckUpdate}
            loading={checking}
          >
            {t('settings.checkUpdate')}
          </Button>
        </div>
        <Divider style={{ margin: '4px 0' }} />
        <div style={rowStyle} className="flex items-center justify-between">
          <span>{t('settings.autoCheckUpdate')}</span>
          <Switch
            checked={autoCheckUpdate}
            onChange={(checked) => saveSettings({ auto_check_update: checked })}
          />
        </div>
        <Divider style={{ margin: '4px 0' }} />
        <div style={rowStyle} className="flex items-center justify-between">
          <span>{t('settings.updateCheckInterval')}</span>
          <InputNumber
            min={1}
            max={1440}
            value={updateCheckInterval}
            disabled={!autoCheckUpdate}
            onChange={(val) => val != null && saveSettings({ update_check_interval: val })}
            style={{ width: 100 }}
            addonAfter={t('settings.minutes')}
          />
        </div>
        {isTauri() && devtoolsEnabled && (
          <>
            <Divider style={{ margin: '4px 0' }} />
            <div style={rowStyle} className="flex items-center justify-between">
              <span>{t('settings.developerTools')}</span>
              <Button
                icon={<Terminal size={16} />}
                onClick={handleOpenDevTools}
              >
                {t('settings.openDevTools')}
              </Button>
            </div>
          </>
        )}
      </SettingsGroup>
    </div>
  );
}
