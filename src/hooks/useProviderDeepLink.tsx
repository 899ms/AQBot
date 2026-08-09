import { Descriptions, Typography } from 'antd';
import { useCallback, useEffect } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { isTauri } from '@/lib/invoke';
import {
  getProviderDeepLinkKeyPrefix,
  parseProviderDeepLink,
  type ProviderDeepLinkPayload,
} from '@/lib/providerDeepLink';
import { useProviderStore, useUIStore } from '@/stores';
import type { DeepLinkProviderImportResult, SettingsSection } from '@/types';

interface ModalLike {
  confirm: (config: {
    title: string;
    content: ReactNode;
    okText: string;
    cancelText: string;
    onOk: () => Promise<void>;
  }) => unknown;
}

interface MessageLike {
  success: (content: string) => unknown;
  error: (content: string) => unknown;
}

interface ConfirmProviderDeepLinkDeps {
  modal: ModalLike;
  message: MessageLike;
  enterSettings: () => void;
  setSettingsSection: (section: SettingsSection) => void;
  setSelectedProviderId: (id: string | null) => void;
  importProvider: (payload: ProviderDeepLinkPayload) => Promise<DeepLinkProviderImportResult>;
  fetchProviders: () => Promise<void>;
  t: (key: string, fallback?: string) => string;
}

function getSuccessMessageKey(result: DeepLinkProviderImportResult): string {
  if (result.created_provider) return 'settings.deepLinkProviderCreated';
  if (result.reused_key) return 'settings.deepLinkProviderReusedKey';
  return 'settings.deepLinkProviderKeyAdded';
}

function ProviderDeepLinkConfirmContent({
  payload,
  t,
}: {
  payload: ProviderDeepLinkPayload;
  t: ConfirmProviderDeepLinkDeps['t'];
}) {
  return (
    <Descriptions size="small" column={1}>
      <Descriptions.Item label={t('common.name')}>{payload.name}</Descriptions.Item>
      <Descriptions.Item label={t('settings.apiHost')}>
        <Typography.Text code>{payload.baseurl}</Typography.Text>
      </Descriptions.Item>
      <Descriptions.Item label={t('settings.providerType')}>{payload.type}</Descriptions.Item>
      <Descriptions.Item label={t('settings.apiKey')}>
        <Typography.Text code>{getProviderDeepLinkKeyPrefix(payload.apikey)}</Typography.Text>
      </Descriptions.Item>
    </Descriptions>
  );
}

export function confirmProviderDeepLinkImport(
  payload: ProviderDeepLinkPayload,
  deps: ConfirmProviderDeepLinkDeps,
) {
  deps.enterSettings();
  deps.setSettingsSection('providers');

  deps.modal.confirm({
    title: deps.t('settings.deepLinkProviderConfirmTitle'),
    content: <ProviderDeepLinkConfirmContent payload={payload} t={deps.t} />,
    okText: deps.t('common.confirm'),
    cancelText: deps.t('common.cancel'),
    onOk: async () => {
      try {
        const result = await deps.importProvider(payload);
        await deps.fetchProviders();
        deps.setSelectedProviderId(result.provider_id);
        deps.message.success(deps.t(getSuccessMessageKey(result)));
      } catch (e) {
        deps.message.error(`${deps.t('settings.deepLinkProviderImportFailed')}: ${String(e)}`);
        throw e;
      }
    },
  });
}

export function useProviderDeepLink({ modal, message }: { modal: ModalLike; message: MessageLike }) {
  const { t } = useTranslation();
  const translate = useCallback((key: string) => t(key), [t]);
  const importProvider = useProviderStore((s) => s.importProviderFromDeepLink);
  const fetchProviders = useProviderStore((s) => s.fetchProviders);
  const enterSettings = useUIStore((s) => s.enterSettings);
  const setSettingsSection = useUIStore((s) => s.setSettingsSection);
  const setSelectedProviderId = useUIStore((s) => s.setSelectedProviderId);

  useEffect(() => {
    if (!isTauri()) return;

    let disposed = false;
    let unlisten: (() => void) | null = null;

    const handleUrls = (urls: string[] | null | undefined) => {
      const payload = urls?.map(parseProviderDeepLink).find((item): item is ProviderDeepLinkPayload => item !== null);
      if (!payload || disposed) return;
      confirmProviderDeepLinkImport(payload, {
        modal,
        message,
        enterSettings,
        setSettingsSection,
        setSelectedProviderId,
        importProvider,
        fetchProviders,
        t: translate,
      });
    };

    const setup = async () => {
      try {
        const { getCurrent, onOpenUrl } = await import('@tauri-apps/plugin-deep-link');
        handleUrls(await getCurrent());
        unlisten = await onOpenUrl(handleUrls);
      } catch (e) {
        console.warn('Failed to initialize deep link listener:', e);
      }
    };

    void setup();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [
    enterSettings,
    fetchProviders,
    importProvider,
    message,
    modal,
    setSelectedProviderId,
    setSettingsSection,
    translate,
  ]);
}
