import {
  Button,
  AutoComplete,
  Input,
  Modal,
  Form,
  Select,
  Switch,
  App,
  theme,
  Divider,
  Dropdown,
  Tooltip,
  Table,
  Tag,
  Typography,
  Empty,
  Checkbox,
  Popconfirm,
  Space,
} from 'antd';
import { Plus, Search, GripVertical, BadgeCheck, Download, ListChecks, X, Power, PowerOff, Trash2 } from 'lucide-react';
import { useCallback, useMemo, useState } from 'react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { useProviderStore, useUIStore } from '@/stores';
import { SmartProviderIcon } from '@/lib/providerIcons';
import type { ProviderConfig, ProviderImportCandidate, ProviderImportStatus, ProviderType } from '@/types';

const PROVIDER_TYPE_OPTIONS: { label: string; value: ProviderType }[] = [
  { label: 'OpenAI', value: 'openai' },
  { label: 'OpenAI Responses', value: 'openai_responses' },
  { label: 'DeepSeek', value: 'deepseek' },
  { label: 'xAI', value: 'xai' },
  { label: 'GLM', value: 'glm' },
  { label: 'SiliconFlow', value: 'siliconflow' },
  { label: 'Anthropic', value: 'anthropic' },
  { label: 'Gemini', value: 'gemini' },
  { label: 'Jina', value: 'jina' },
  { label: 'Cohere', value: 'cohere' },
  { label: 'Voyage', value: 'voyage' },
  { label: 'AWS Bedrock', value: 'bedrock' },
];

const AWS_REGION_OPTIONS = [
  'us-east-1',
  'us-east-2',
  'us-west-2',
  'eu-central-1',
  'eu-west-1',
  'eu-west-2',
  'eu-west-3',
  'ap-northeast-1',
  'ap-northeast-2',
  'ap-southeast-1',
  'ap-southeast-2',
].map((value) => ({ value }));

const DEFAULT_HOSTS: Record<ProviderType, string> = {
  openai: 'https://api.openai.com',
  openai_responses: 'https://api.openai.com',
  deepseek: 'https://api.deepseek.com',
  xai: 'https://api.x.ai',
  glm: 'https://open.bigmodel.cn/api/paas',
  siliconflow: 'https://api.siliconflow.cn',
  anthropic: 'https://api.anthropic.com',
  gemini: 'https://generativelanguage.googleapis.com',
  jina: 'https://api.jina.ai',
  cohere: 'https://api.cohere.com',
  voyage: 'https://api.voyageai.com',
  bedrock: '',
  custom: '',
};

const IMPORTABLE_STATUSES = new Set<ProviderImportStatus>(['ready', 'add_key']);

function getImportStatusColor(status: ProviderImportStatus) {
  switch (status) {
    case 'ready':
      return 'green';
    case 'add_key':
      return 'blue';
    case 'already_exists':
      return 'default';
    case 'unsupported':
      return 'orange';
    default:
      return 'default';
  }
}

function BuiltinProviderIcon({
  provider,
  token,
  label,
}: {
  provider: ProviderConfig;
  token: any;
  label: string;
}) {
  if (!provider.builtin_id) {
    return <SmartProviderIcon provider={provider} size={22} type="avatar" />;
  }

  return (
    <span
      style={{
        position: 'relative',
        display: 'inline-flex',
        alignItems: 'center',
        flexShrink: 0,
        width: 26,
        height: 22,
      }}
    >
      <SmartProviderIcon provider={provider} size={22} type="avatar" />
      <span
        role="img"
        aria-label={label}
        title={label}
        style={{
          position: 'absolute',
          top: -4,
          right: -4,
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          boxSizing: 'border-box',
          width: 12,
          height: 12,
          borderRadius: '50%',
          color: token.colorPrimary,
          background: token.colorPrimaryBg,
          border: `1px solid ${token.colorBgContainer}`,
          pointerEvents: 'none',
        }}
      >
        <BadgeCheck size={8} strokeWidth={3} aria-hidden />
      </span>
    </span>
  );
}

function isBuiltinProvider(provider: ProviderConfig) {
  return Boolean(provider.builtin_id || provider.id.startsWith('builtin_'));
}

function SortableProviderItem({
  provider,
  isSelected,
  token,
  onSelect,
  onToggle,
  onRequestDelete,
  batchMode,
  batchChecked,
  onBatchCheck,
}: {
  provider: ProviderConfig;
  isSelected: boolean;
  token: any;
  onSelect: () => void;
  onToggle: (checked: boolean) => void;
  onRequestDelete: () => void;
  batchMode: boolean;
  batchChecked: boolean;
  onBatchCheck: (checked: boolean) => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: provider.id, disabled: batchMode });
  const { t } = useTranslation();

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
    borderRadius: token.borderRadius,
    backgroundColor: isSelected || (batchMode && batchChecked) ? token.colorPrimaryBg : undefined,
  };

  const disabled = !provider.enabled;
  const isBuiltin = isBuiltinProvider(provider);

  const contextMenuItems = useMemo(() => {
    const items: Array<{
      key: string;
      label: string;
      icon: React.ReactNode;
      danger?: boolean;
    }> = [
      provider.enabled
        ? {
            key: 'disable',
            label: t('settings.disableProvider'),
            icon: <PowerOff size={14} />,
          }
        : {
            key: 'enable',
            label: t('settings.enableProvider'),
            icon: <Power size={14} />,
          },
    ];
    if (!isBuiltin) {
      items.push({
        key: 'delete',
        label: t('settings.deleteProvider'),
        icon: <Trash2 size={14} />,
        danger: true,
      });
    }
    return items;
  }, [provider.enabled, isBuiltin, t]);

  const row = (
    <div
      ref={setNodeRef}
      style={style}
      className="flex items-center cursor-pointer px-3 py-2.5 transition-colors"
      onClick={() => {
        if (batchMode) {
          onBatchCheck(!batchChecked);
          return;
        }
        onSelect();
      }}
      onMouseEnter={(e) => {
        if (!isSelected && !(batchMode && batchChecked)) {
          e.currentTarget.style.backgroundColor = token.colorFillQuaternary;
        }
      }}
      onMouseLeave={(e) => {
        if (!isSelected && !(batchMode && batchChecked)) {
          e.currentTarget.style.backgroundColor = '';
        }
      }}
    >
      {batchMode ? (
        <div
          className="flex items-center mr-2"
          onClick={(e) => e.stopPropagation()}
        >
          <Checkbox
            checked={batchChecked}
            onChange={(e) => onBatchCheck(e.target.checked)}
            aria-label={t('common.select')}
          />
        </div>
      ) : (
        <div
          {...attributes}
          {...listeners}
          className="flex items-center mr-2 cursor-grab"
          style={{ color: token.colorTextQuaternary }}
          onClick={(e) => e.stopPropagation()}
        >
          <GripVertical size={14} />
        </div>
      )}
      <div
        className="min-w-0 flex-1 flex items-center gap-2"
        style={{ opacity: disabled ? 0.4 : 1 }}
      >
        <BuiltinProviderIcon provider={provider} token={token} label={t('settings.builtinProviderBadge')} />
        <span style={{ color: isSelected || (batchMode && batchChecked) ? token.colorPrimary : undefined }}>{provider.name}</span>
      </div>
      {!batchMode && (
        <Switch
          size="small"
          checked={provider.enabled}
          onClick={(_, e) => e.stopPropagation()}
          onChange={onToggle}
        />
      )}
    </div>
  );

  if (batchMode) {
    return row;
  }

  return (
    <Dropdown
      trigger={['contextMenu']}
      menu={{
        items: contextMenuItems,
        onClick: ({ key, domEvent }) => {
          domEvent.stopPropagation();
          if (key === 'enable') onToggle(true);
          else if (key === 'disable') onToggle(false);
          else if (key === 'delete') onRequestDelete();
        },
      }}
    >
      {row}
    </Dropdown>
  );
}

export function ProviderList() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { message, modal } = App.useApp();
  const providers = useProviderStore((s) => s.providers);
  const createProvider = useProviderStore((s) => s.createProvider);
  const scanCcSwitchProviderImports = useProviderStore((s) => s.scanCcSwitchProviderImports);
  const importCcSwitchProviderConfigs = useProviderStore((s) => s.importCcSwitchProviderConfigs);
  const toggleProvider = useProviderStore((s) => s.toggleProvider);
  const deleteProvider = useProviderStore((s) => s.deleteProvider);
  const reorderProviders = useProviderStore((s) => s.reorderProviders);
  const selectedProviderId = useUIStore((s) => s.selectedProviderId);
  const setSelectedProviderId = useUIStore((s) => s.setSelectedProviderId);

  const handleRequestDeleteProvider = useCallback((providerId: string) => {
    modal.confirm({
      title: t('settings.deleteProviderConfirm'),
      mask: { enabled: true, blur: true },
      okText: t('common.confirm'),
      cancelText: t('common.cancel'),
      okButtonProps: { danger: true },
      onOk: async () => {
        await deleteProvider(providerId);
        if (selectedProviderId === providerId) {
          const remaining = providers.filter((p) => p.id !== providerId);
          setSelectedProviderId(remaining[0]?.id ?? null);
        }
      },
    });
  }, [modal, t, deleteProvider, selectedProviderId, providers, setSelectedProviderId]);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );

  // Preserve the logical selection when a virtual built-in receives its real database ID.
  React.useEffect(() => {
    if (providers.length === 0) return;
    if (selectedProviderId && providers.some((provider) => provider.id === selectedProviderId)) {
      return;
    }

    const materializedProvider = providers.find(
      (provider) =>
        provider.builtin_id
        && selectedProviderId === `builtin_${provider.builtin_id}`,
    );
    setSelectedProviderId(materializedProvider?.id ?? providers[0].id);
  }, [selectedProviderId, providers, setSelectedProviderId]);

  const [search, setSearch] = useState('');
  const [modalOpen, setModalOpen] = useState(false);
  const [importModalOpen, setImportModalOpen] = useState(false);
  const [importScanning, setImportScanning] = useState(false);
  const [importSubmitting, setImportSubmitting] = useState(false);
  const [importCandidates, setImportCandidates] = useState<ProviderImportCandidate[]>([]);
  const [selectedImportIds, setSelectedImportIds] = useState<React.Key[]>([]);
  const [batchMode, setBatchMode] = useState(false);
  const [batchSelected, setBatchSelected] = useState<Set<string>>(new Set());
  const [batchBusy, setBatchBusy] = useState(false);
  const [form] = Form.useForm();
  const selectedProviderType = Form.useWatch<ProviderType>('provider_type', form);

  const filteredProviders = useMemo(
    () =>
      providers.filter((p) =>
        p.name.toLowerCase().includes(search.toLowerCase()),
      ),
    [providers, search],
  );

  const enabledProviders = useMemo(
    () => filteredProviders.filter((p) => p.enabled),
    [filteredProviders],
  );

  const disabledProviders = useMemo(
    () => filteredProviders.filter((p) => !p.enabled),
    [filteredProviders],
  );

  const handleEnterBatchMode = useCallback(() => {
    setBatchMode(true);
    setBatchSelected(new Set());
  }, []);

  const handleExitBatchMode = useCallback(() => {
    setBatchMode(false);
    setBatchSelected(new Set());
  }, []);

  const handleBatchCheck = useCallback((providerId: string, checked: boolean) => {
    setBatchSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(providerId);
      else next.delete(providerId);
      return next;
    });
  }, []);

  const handleBatchToggleAll = useCallback((checked: boolean) => {
    setBatchSelected((prev) => {
      const next = new Set(prev);
      for (const provider of filteredProviders) {
        if (checked) next.add(provider.id);
        else next.delete(provider.id);
      }
      return next;
    });
  }, [filteredProviders]);

  const handleBatchEnable = useCallback(async () => {
    if (batchSelected.size === 0 || batchBusy) return;
    setBatchBusy(true);
    let ok = 0;
    try {
      for (const id of batchSelected) {
        const provider = providers.find((p) => p.id === id);
        if (!provider || provider.enabled) continue;
        try {
          await toggleProvider(id, true);
          ok += 1;
        } catch {
          // continue remaining
        }
      }
      if (ok > 0) {
        message.success(t('settings.providerBatchEnableSuccess', { count: ok }));
      }
    } finally {
      setBatchBusy(false);
    }
  }, [batchSelected, batchBusy, providers, toggleProvider, message, t]);

  const handleBatchDisable = useCallback(async () => {
    if (batchSelected.size === 0 || batchBusy) return;
    setBatchBusy(true);
    let ok = 0;
    try {
      for (const id of batchSelected) {
        const provider = providers.find((p) => p.id === id);
        if (!provider || !provider.enabled) continue;
        try {
          await toggleProvider(id, false);
          ok += 1;
        } catch {
          // continue remaining
        }
      }
      if (ok > 0) {
        message.success(t('settings.providerBatchDisableSuccess', { count: ok }));
      }
    } finally {
      setBatchBusy(false);
    }
  }, [batchSelected, batchBusy, providers, toggleProvider, message, t]);

  const handleBatchDelete = useCallback(async () => {
    if (batchSelected.size === 0 || batchBusy) return;
    const selectedProviders = providers.filter((p) => batchSelected.has(p.id));
    const deletable = selectedProviders.filter((p) => !isBuiltinProvider(p));
    const skipped = selectedProviders.length - deletable.length;
    if (deletable.length === 0) {
      if (skipped > 0) {
        message.warning(t('settings.providerBatchSkipBuiltin', { count: skipped }));
      }
      return;
    }
    setBatchBusy(true);
    let ok = 0;
    const deletedIds = new Set<string>();
    try {
      for (const provider of deletable) {
        try {
          await deleteProvider(provider.id);
          deletedIds.add(provider.id);
          ok += 1;
        } catch {
          // continue remaining
        }
      }
      if (ok > 0) {
        message.success(t('settings.providerBatchDeleteSuccess', { count: ok }));
      }
      if (skipped > 0) {
        message.warning(t('settings.providerBatchSkipBuiltin', { count: skipped }));
      }
      setBatchSelected((prev) => {
        const next = new Set(prev);
        for (const id of deletedIds) next.delete(id);
        return next;
      });
      if (selectedProviderId && deletedIds.has(selectedProviderId)) {
        const remaining = providers.filter((p) => !deletedIds.has(p.id));
        setSelectedProviderId(remaining[0]?.id ?? null);
      }
    } finally {
      setBatchBusy(false);
    }
  }, [batchSelected, batchBusy, providers, deleteProvider, selectedProviderId, setSelectedProviderId, message, t]);

  const allFilteredSelected =
    filteredProviders.length > 0
    && filteredProviders.every((p) => batchSelected.has(p.id));
  const someFilteredSelected =
    filteredProviders.some((p) => batchSelected.has(p.id))
    && !allFilteredSelected;
  const deletableSelectedCount = providers.filter(
    (p) => batchSelected.has(p.id) && !isBuiltinProvider(p),
  ).length;

  const importColumns = useMemo(
    () => [
      {
        title: t('settings.ccSwitchImportSourceApp'),
        dataIndex: 'source_app',
        key: 'source_app',
        width: 120,
      },
      {
        title: t('settings.ccSwitchImportProvider'),
        dataIndex: 'name',
        key: 'name',
        width: 160,
      },
      {
        title: t('settings.ccSwitchImportType'),
        dataIndex: 'provider_type',
        key: 'provider_type',
        width: 140,
        render: (value: ProviderType) => PROVIDER_TYPE_OPTIONS.find((item) => item.value === value)?.label ?? value,
      },
      {
        title: t('settings.ccSwitchImportEndpoint'),
        key: 'endpoint',
        width: 260,
        render: (_: unknown, candidate: ProviderImportCandidate) => (
          <Typography.Text ellipsis style={{ maxWidth: 240 }}>
            {candidate.api_host}
            {candidate.api_path ?? ''}
          </Typography.Text>
        ),
      },
      {
        title: t('settings.ccSwitchImportKey'),
        dataIndex: 'key_prefix',
        key: 'key_prefix',
        width: 110,
        render: (value: string) => value || '-',
      },
      {
        title: t('settings.ccSwitchImportModels'),
        key: 'models',
        width: 90,
        align: 'right' as const,
        render: (_: unknown, candidate: ProviderImportCandidate) => candidate.models.length,
      },
      {
        title: t('settings.ccSwitchImportStatus'),
        key: 'status',
        width: 180,
        render: (_: unknown, candidate: ProviderImportCandidate) => (
          <div className="flex flex-col gap-1">
            <Tag color={getImportStatusColor(candidate.status)} style={{ marginInlineEnd: 0, width: 'fit-content' }}>
              {t(`settings.ccSwitchImportStatus_${candidate.status}`)}
            </Tag>
            {candidate.reason && (
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {candidate.reason}
              </Typography.Text>
            )}
          </div>
        ),
      },
    ],
    [t],
  );

  const handleAddProvider = async () => {
    try {
      const values = await form.validateFields();
      const providerType = values.provider_type as ProviderType;
      const isBedrock = providerType === 'bedrock';
      const provider = await createProvider({
        name: values.name,
        provider_type: providerType,
        api_host: isBedrock ? '' : values.api_host || DEFAULT_HOSTS[providerType],
        aws_region: isBedrock ? values.aws_region.trim() : null,
        enabled: true,
      });
      setSelectedProviderId(provider.id);
      setModalOpen(false);
      form.resetFields();
    } catch (e) {
      if (e && typeof e === 'object' && 'errorFields' in e) return;
      message.error(t('error.saveFailed'));
    }
  };

  const handleScanCcSwitch = async () => {
    setImportScanning(true);
    try {
      const candidates = await scanCcSwitchProviderImports();
      setImportCandidates(candidates);
      setSelectedImportIds(
        candidates
          .filter((candidate) => IMPORTABLE_STATUSES.has(candidate.status))
          .map((candidate) => candidate.id),
      );
      setImportModalOpen(true);
    } catch (e) {
      message.error(t('settings.ccSwitchImportScanFailed', { reason: String(e) }));
    } finally {
      setImportScanning(false);
    }
  };

  const handleImportCandidates = async () => {
    if (selectedImportIds.length === 0) {
      return;
    }
    setImportSubmitting(true);
    try {
      const result = await importCcSwitchProviderConfigs(selectedImportIds.map(String));
      message.success(
        t('settings.ccSwitchImportSuccess', {
          created: result.created_count,
          added: result.added_key_count,
          reused: result.reused_count,
          skipped: result.skipped_count,
        }),
      );
      if (result.provider_ids.length > 0) {
        setSelectedProviderId(result.provider_ids[0]);
      }
      setImportModalOpen(false);
      setImportCandidates([]);
      setSelectedImportIds([]);
    } catch (e) {
      message.error(t('settings.ccSwitchImportFailed', { reason: String(e) }));
    } finally {
      setImportSubmitting(false);
    }
  };

  const handleTypeChange = (type: ProviderType) => {
    form.setFieldValue('api_host', DEFAULT_HOSTS[type]);
    if (type !== 'bedrock') form.setFieldValue('aws_region', undefined);
  };

  const handleDragEnd = (sectionProviders: ProviderConfig[]) => (event: DragEndEvent) => {
    const { active, over } = event;
    if (over && active.id !== over.id) {
      const ids = sectionProviders.map((p) => p.id);
      const oldIndex = ids.indexOf(String(active.id));
      const newIndex = ids.indexOf(String(over.id));
      if (oldIndex !== -1 && newIndex !== -1) {
        const newIds = [...ids];
        newIds.splice(oldIndex, 1);
        newIds.splice(newIndex, 0, String(active.id));
        // Build full reorder: reordered section + other section
        const otherIds = (sectionProviders === enabledProviders ? disabledProviders : enabledProviders).map((p) => p.id);
        const fullIds = sectionProviders === enabledProviders
          ? [...newIds, ...otherIds]
          : [...otherIds, ...newIds];
        reorderProviders(fullIds);
      }
    }
  };

  const sectionHeaderStyle: React.CSSProperties = {
    fontSize: 11,
    fontWeight: 600,
    color: token.colorTextTertiary,
    textTransform: 'uppercase',
    letterSpacing: 0.5,
    padding: '4px 12px 2px',
  };

  return (
    <div className="flex h-full flex-col">
      <div className="p-3 flex items-center gap-2">
        {batchMode ? (
          <>
            <Checkbox
              aria-label={t('common.selectAll')}
              checked={allFilteredSelected}
              indeterminate={someFilteredSelected}
              disabled={filteredProviders.length === 0 || batchBusy}
              onChange={(e) => handleBatchToggleAll(e.target.checked)}
            >
              <Typography.Text type="secondary" style={{ fontSize: 12, whiteSpace: 'nowrap' }}>
                {t('settings.providerBatchSelected', {
                  count: batchSelected.size,
                })}
              </Typography.Text>
            </Checkbox>
            <div className="flex-1" />
            <Space size={2}>
              <Tooltip title={t('settings.batchEnable')}>
                <Button
                  type="text"
                  size="small"
                  icon={<Power size={14} />}
                  disabled={batchSelected.size === 0 || batchBusy}
                  loading={batchBusy}
                  onClick={() => void handleBatchEnable()}
                  aria-label={t('settings.batchEnable')}
                />
              </Tooltip>
              <Tooltip title={t('settings.batchDisable')}>
                <Button
                  type="text"
                  size="small"
                  icon={<PowerOff size={14} />}
                  disabled={batchSelected.size === 0 || batchBusy}
                  loading={batchBusy}
                  onClick={() => void handleBatchDisable()}
                  aria-label={t('settings.batchDisable')}
                />
              </Tooltip>
              <Popconfirm
                title={t('settings.providerBatchDeleteConfirm', {
                  count: deletableSelectedCount,
                })}
                onConfirm={() => void handleBatchDelete()}
                okText={t('common.confirm')}
                cancelText={t('common.cancel')}
                okButtonProps={{ danger: true }}
                disabled={deletableSelectedCount === 0 || batchBusy}
              >
                <Tooltip title={t('settings.batchDeleteBtn')}>
                  <Button
                    type="text"
                    size="small"
                    danger
                    icon={<Trash2 size={14} />}
                    disabled={deletableSelectedCount === 0 || batchBusy}
                    aria-label={t('settings.batchDeleteBtn')}
                  />
                </Tooltip>
              </Popconfirm>
              <Divider type="vertical" style={{ margin: '0 2px' }} />
              <Tooltip title={t('settings.batchExit')}>
                <Button
                  type="text"
                  size="small"
                  icon={<X size={14} />}
                  onClick={handleExitBatchMode}
                  disabled={batchBusy}
                  aria-label={t('settings.batchExit')}
                />
              </Tooltip>
            </Space>
          </>
        ) : (
          <>
            <Input
              prefix={<Search size={14} />}
              placeholder={t('settings.filterProviders')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              allowClear
              style={{ flex: 1 }}
            />
            <Tooltip title={t('settings.providerBatchMode')}>
              <Button
                type="default"
                aria-label={t('settings.providerBatchMode')}
                icon={<ListChecks size={16} />}
                onClick={handleEnterBatchMode}
                style={{ flexShrink: 0 }}
              />
            </Tooltip>
            <Button
              type="default"
              aria-label={t('settings.addProvider')}
              icon={<Plus size={16} />}
              onClick={() => setModalOpen(true)}
              style={{ flexShrink: 0 }}
            />
            <Dropdown
              menu={{
                items: [
                  {
                    key: 'cc-switch',
                    label: t('settings.importFromCcSwitch'),
                    onClick: handleScanCcSwitch,
                  },
                ],
              }}
              trigger={['click']}
            >
              <Tooltip title={t('settings.importProviders')}>
                <Button
                  type="default"
                  aria-label={t('settings.importProviders')}
                  icon={<Download size={16} />}
                  loading={importScanning}
                  style={{ flexShrink: 0 }}
                />
              </Tooltip>
            </Dropdown>
          </>
        )}
      </div>
      <div className="flex-1 overflow-y-auto p-2 flex flex-col gap-0">
        {enabledProviders.length > 0 && (
          <>
            <div style={sectionHeaderStyle}>{t('settings.enabledProviders')}</div>
            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              onDragEnd={handleDragEnd(enabledProviders)}
            >
              <SortableContext
                items={enabledProviders.map((p) => p.id)}
                strategy={verticalListSortingStrategy}
              >
                <div className="flex flex-col gap-1">
                  {enabledProviders.map((provider) => (
                    <SortableProviderItem
                      key={provider.id}
                      provider={provider}
                      isSelected={selectedProviderId === provider.id}
                      token={token}
                      onSelect={() => setSelectedProviderId(provider.id)}
                      onToggle={(checked) => toggleProvider(provider.id, checked)}
                      onRequestDelete={() => handleRequestDeleteProvider(provider.id)}
                      batchMode={batchMode}
                      batchChecked={batchSelected.has(provider.id)}
                      onBatchCheck={(checked) => handleBatchCheck(provider.id, checked)}
                    />
                  ))}
                </div>
              </SortableContext>
            </DndContext>
          </>
        )}

        {enabledProviders.length > 0 && disabledProviders.length > 0 && (
          <Divider style={{ margin: '8px 0' }} />
        )}

        {disabledProviders.length > 0 && (
          <>
            <div style={sectionHeaderStyle}>{t('settings.disabledProviders')}</div>
            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              onDragEnd={handleDragEnd(disabledProviders)}
            >
              <SortableContext
                items={disabledProviders.map((p) => p.id)}
                strategy={verticalListSortingStrategy}
              >
                <div className="flex flex-col gap-1">
                  {disabledProviders.map((provider) => (
                    <SortableProviderItem
                      key={provider.id}
                      provider={provider}
                      isSelected={selectedProviderId === provider.id}
                      onSelect={() => setSelectedProviderId(provider.id)}
                      token={token}
                      onToggle={(checked) => toggleProvider(provider.id, checked)}
                      onRequestDelete={() => handleRequestDeleteProvider(provider.id)}
                      batchMode={batchMode}
                      batchChecked={batchSelected.has(provider.id)}
                      onBatchCheck={(checked) => handleBatchCheck(provider.id, checked)}
                    />
                  ))}
                </div>
              </SortableContext>
            </DndContext>
          </>
        )}
      </div>

      <Modal
        title={t('settings.addProvider')}
        open={modalOpen}
        mask={{ enabled: true, blur: true }}
        onOk={handleAddProvider}
        onCancel={() => {
          setModalOpen(false);
          form.resetFields();
        }}
        okText={t('common.confirm')}
        cancelText={t('common.cancel')}
      >
        <Form form={form} layout="vertical">
          <Form.Item
            name="name"
            label={t('settings.providerName')}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="provider_type"
            label={t('settings.providerType')}
            rules={[{ required: true }]}
          >
            <Select options={PROVIDER_TYPE_OPTIONS} onChange={handleTypeChange} />
          </Form.Item>
          {selectedProviderType === 'bedrock' ? (
            <Form.Item
              name="aws_region"
              label={t('settings.awsRegion')}
              rules={[
                { required: true, whitespace: true, message: t('settings.awsRegionRequired') },
              ]}
            >
              <AutoComplete
                options={AWS_REGION_OPTIONS}
                placeholder="us-east-1"
                filterOption={(input, option) =>
                  String(option?.value ?? '').toLowerCase().includes(input.toLowerCase())
                }
              />
            </Form.Item>
          ) : (
            <Form.Item name="api_host" label={t('settings.apiHost')}>
              <Input placeholder="https://api.openai.com" />
            </Form.Item>
          )}
        </Form>
      </Modal>

      <Modal
        title={t('settings.ccSwitchImportTitle')}
        open={importModalOpen}
        mask={{ enabled: true, blur: true }}
        width={960}
        onOk={handleImportCandidates}
        onCancel={() => {
          setImportModalOpen(false);
          setImportCandidates([]);
          setSelectedImportIds([]);
        }}
        okText={t('common.confirm')}
        cancelText={t('common.cancel')}
        okButtonProps={{ disabled: selectedImportIds.length === 0 }}
        confirmLoading={importSubmitting}
      >
        {importCandidates.length === 0 ? (
          <Empty description={t('settings.ccSwitchImportEmpty')} />
        ) : (
          <Table
            size="small"
            rowKey="id"
            pagination={false}
            dataSource={importCandidates}
            columns={importColumns}
            scroll={{ x: 920 }}
            rowSelection={{
              selectedRowKeys: selectedImportIds,
              onChange: setSelectedImportIds,
              getCheckboxProps: (candidate) => ({
                disabled: candidate.status === 'unsupported',
              }),
            }}
          />
        )}
      </Modal>
    </div>
  );
}
