import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Alert,
  Button,
  Divider,
  Input,
  Modal,
  Select,
  Steps,
  Switch,
  Tag,
  Tooltip,
  message,
  theme,
} from 'antd';
import { GripVertical, Plus, RotateCcw, Trash2 } from 'lucide-react';
import {
  DndContext,
  PointerSensor,
  closestCenter,
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
import { useTranslation } from 'react-i18next';
import { invoke } from '@/lib/invoke';
import { useProviderStore, useSettingsStore } from '@/stores';
import { ModelParamSliders } from '@/components/common/ModelParamSliders';
import { ModelSelect, parseModelValue } from '@/components/shared/ModelSelect';
import { LucideToolIcon } from '@/components/shared/LucideToolIcon';
import {
  SELECTION_TRANSLATE_LANGUAGES,
  type SelectionTranslateLanguage,
} from '@/constants/selectionTranslateLanguages';
import {
  createDefaultSelectionToolbarSettings,
  type SelectionToolbarPermissionSettingsOutcome,
  type SelectionToolbarRuntimeStatus,
  type SelectionToolbarSettings as SelectionToolbarConfig,
  type SelectionToolbarTool,
} from '@/types';
import { SettingsGroup } from './SettingsGroup';

const LucideIconPickerModal = lazy(() => import('@/components/shared/LucideIconPickerModal'));

const { TextArea } = Input;

function toolId(tool: SelectionToolbarTool) {
  return tool.kind === 'custom_ai' ? tool.id : tool.builtin_key;
}

function toolIconName(tool: SelectionToolbarTool): string {
  if (tool.kind === 'builtin_action') return 'copy';
  if (tool.kind === 'builtin_ai') {
    return {
      translate: 'languages',
      polish: 'spell-check',
      summarize: 'list-collapse',
    }[tool.builtin_key];
  }
  return tool.icon;
}

function toolName(tool: SelectionToolbarTool, t: (key: string) => string) {
  return tool.kind === 'custom_ai'
    ? tool.name
    : t(`settings.selectionToolbar.tools.${tool.builtin_key}`);
}

function SortableToolRow({
  tool,
  onToggle,
  onEdit,
  onReset,
  onDelete,
}: {
  tool: SelectionToolbarTool;
  onToggle: (enabled: boolean) => void;
  onEdit: () => void;
  onReset: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const sortable = useSortable({ id: toolId(tool) });

  return (
    <div
      ref={sortable.setNodeRef}
      style={{
        alignItems: 'center',
        display: 'flex',
        gap: 10,
        opacity: sortable.isDragging ? 0.5 : 1,
        padding: '10px 4px',
        transform: CSS.Transform.toString(sortable.transform),
        transition: sortable.transition,
      }}
    >
      <button
        aria-label={t('settings.selectionToolbar.reorder')}
        {...sortable.attributes}
        {...sortable.listeners}
        style={{
          background: 'none',
          border: 0,
          color: token.colorTextQuaternary,
          cursor: sortable.isDragging ? 'grabbing' : 'grab',
          padding: 2,
          touchAction: 'none',
        }}
        type="button"
      >
        <GripVertical size={16} />
      </button>
      <LucideToolIcon name={toolIconName(tool)} size={18} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ alignItems: 'center', display: 'flex', gap: 6 }}>
          <span style={{ fontWeight: 500 }}>{toolName(tool, t)}</span>
          <Tag bordered={false}>
            {t(`settings.selectionToolbar.${tool.kind === 'custom_ai' ? 'custom' : 'builtin'}`)}
          </Tag>
        </div>
        <div style={{ color: token.colorTextDescription, fontSize: 12 }}>
          {t(`settings.selectionToolbar.${tool.kind === 'builtin_action' ? 'actionTool' : 'aiTool'}`)}
        </div>
      </div>
      {tool.kind !== 'builtin_action' && (
        <Button size="small" type="text" onClick={onEdit}>
          {t('common.edit')}
        </Button>
      )}
      {tool.kind !== 'custom_ai' && (
        <Tooltip title={t('settings.selectionToolbar.reset')}>
          <Button aria-label={t('settings.selectionToolbar.reset')} icon={<RotateCcw size={14} />} size="small" type="text" onClick={onReset} />
        </Tooltip>
      )}
      {tool.kind === 'custom_ai' && (
        <Tooltip title={t('common.delete')}>
          <Button aria-label={t('common.delete')} danger icon={<Trash2 size={14} />} size="small" type="text" onClick={onDelete} />
        </Tooltip>
      )}
      <Switch aria-label={toolName(tool, t)} checked={tool.enabled} size="small" onChange={onToggle} />
    </div>
  );
}

function ToolEditor({
  tool,
  onClose,
  onSave,
}: {
  tool: SelectionToolbarTool | null;
  onClose: () => void;
  onSave: (tool: SelectionToolbarTool) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<SelectionToolbarTool | null>(tool);
  const [iconPickerOpen, setIconPickerOpen] = useState(false);

  useEffect(() => {
    setDraft(tool);
    setIconPickerOpen(false);
  }, [tool]);
  if (!draft || draft.kind === 'builtin_action') return null;

  const modelValue = draft.ai.provider_id && draft.ai.model_id
    ? `${draft.ai.provider_id}::${draft.ai.model_id}`
    : undefined;

  const submit = () => {
    if (draft.kind === 'custom_ai' && !draft.name.trim()) {
      message.error(t('settings.selectionToolbar.nameRequired'));
      return;
    }
    if (!draft.ai.prompt.includes('{selection}')) {
      message.error(t('settings.selectionToolbar.placeholderRequired'));
      return;
    }
    onSave(draft);
  };

  return (
    <Modal
      footer={[
        <Button key="cancel" onClick={onClose}>{t('common.cancel')}</Button>,
        <Button key="save" type="primary" onClick={submit}>{t('common.save')}</Button>,
      ]}
      mask={{ enabled: true, blur: true }}
      open
      title={toolName(draft, t)}
      width={560}
      onCancel={onClose}
    >
      {draft.kind === 'custom_ai' && (
        <div style={{ display: 'grid', gap: 12, gridTemplateColumns: '1fr auto', marginBottom: 16 }}>
          <Input
            aria-label={t('settings.selectionToolbar.name')}
            value={draft.name}
            onChange={(event) => setDraft({ ...draft, name: event.target.value })}
          />
          <Button
            aria-label={t('settings.selectionToolbar.icon')}
            icon={<LucideToolIcon name={draft.icon} size={16} />}
            title={draft.icon}
            onClick={() => setIconPickerOpen(true)}
          >
            {t('settings.selectionToolbar.icon')}
          </Button>
          {iconPickerOpen && (
            <Suspense fallback={null}>
              <LucideIconPickerModal
                open
                value={draft.icon}
                onClose={() => setIconPickerOpen(false)}
                onSelect={(icon) => setDraft({ ...draft, icon })}
              />
            </Suspense>
          )}
        </div>
      )}
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
        {t('settings.selectionToolbar.prompt')}
      </div>
      <TextArea
        aria-label={t('settings.selectionToolbar.prompt')}
        rows={6}
        value={draft.ai.prompt}
        onChange={(event) => setDraft({ ...draft, ai: { ...draft.ai, prompt: event.target.value } })}
      />
      <div style={{ color: 'var(--text-color-secondary)', fontSize: 12, margin: '6px 0 16px' }}>
        {t('settings.selectionToolbar.promptHint')}
      </div>
      <ModelSelect
        modelType="Chat"
        placeholder={t('settings.selectionToolbar.inheritModel')}
        style={{ width: '100%' }}
        value={modelValue}
        onChange={(value) => {
          const parsed = parseModelValue(value);
          setDraft({
            ...draft,
            ai: {
              ...draft.ai,
              provider_id: parsed?.providerId ?? null,
              model_id: parsed?.modelId ?? null,
            },
          });
        }}
      />
      <Divider />
      <ModelParamSliders
        values={{
          temperature: draft.ai.temperature,
          topP: draft.ai.top_p,
          maxTokens: draft.ai.max_tokens,
          frequencyPenalty: null,
        }}
        visibleParams={['temperature', 'topP', 'maxTokens']}
        onChange={(values) => setDraft({
          ...draft,
          ai: {
            ...draft.ai,
            ...('temperature' in values ? { temperature: values.temperature ?? null } : {}),
            ...('topP' in values ? { top_p: values.topP ?? null } : {}),
            ...('maxTokens' in values ? { max_tokens: values.maxTokens ?? null } : {}),
          },
        })}
      />
    </Modal>
  );
}

export function SelectionToolbarSettings() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const settings = useSettingsStore((state) => state.settings.selection_toolbar);
  const saveSettings = useSettingsStore((state) => state.saveSettings);
  const ensureProvidersLoaded = useProviderStore((state) => state.ensureProvidersLoaded);
  const [runtime, setRuntime] = useState<SelectionToolbarRuntimeStatus | null>(null);
  const [editing, setEditing] = useState<SelectionToolbarTool | null>(null);
  const [manualPermissionPath, setManualPermissionPath] = useState<string | null>(null);
  const [permissionGuideOpen, setPermissionGuideOpen] = useState(false);
  const runtimeRefreshInFlight = useRef<Promise<SelectionToolbarRuntimeStatus> | null>(null);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));

  const reportRuntimeError = useCallback((error: unknown) => setRuntime({
    state: 'error',
    platform: 'unsupported',
    permission: 'unknown',
    last_error: { code: 'status_failed', message: String(error) },
    global_dismissal_supported: false,
  }), []);

  const refreshRuntime = useCallback(() => {
    if (runtimeRefreshInFlight.current) return runtimeRefreshInFlight.current;
    const request = (async () => {
      let next = await invoke<SelectionToolbarRuntimeStatus>(
        'selection_toolbar_get_runtime_status',
      );
      if (
        settings.enabled
        && next.permission === 'granted'
        && next.state === 'permission_required'
      ) {
        next = await invoke<SelectionToolbarRuntimeStatus>(
          'selection_toolbar_retry_monitoring',
        );
      }
      setRuntime(next);
      return next;
    })();
    runtimeRefreshInFlight.current = request;
    const clearRequest = () => {
      if (runtimeRefreshInFlight.current === request) {
        runtimeRefreshInFlight.current = null;
      }
    };
    void request.then(clearRequest, clearRequest);
    return request;
  }, [settings.enabled]);

  useEffect(() => {
    void ensureProvidersLoaded();
  }, [ensureProvidersLoaded]);

  useEffect(() => {
    const refresh = () => {
      void refreshRuntime().catch(reportRuntimeError);
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === 'visible') refresh();
    };

    void refreshRuntime()
      .catch(reportRuntimeError);
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', refreshWhenVisible);
    return () => {
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', refreshWhenVisible);
    };
  }, [refreshRuntime, reportRuntimeError]);

  useEffect(() => {
    if (runtime?.platform !== 'macos') return;
    const interval = window.setInterval(() => {
      void refreshRuntime().catch(reportRuntimeError);
    }, 1500);
    return () => window.clearInterval(interval);
  }, [refreshRuntime, reportRuntimeError, runtime?.platform]);

  const persist = async (next: SelectionToolbarConfig) => {
    try {
      await saveSettings({ selection_toolbar: next });
      const state = useSettingsStore.getState();
      if (state.settings.selection_toolbar !== next) {
        message.error(state.error ?? t('settings.selectionToolbar.saveFailed'));
        return;
      }
      await refreshRuntime();
    } catch (error) {
      message.error(String(error));
    }
  };
  const ids = useMemo(() => settings.tools.map(toolId), [settings.tools]);
  const permission = runtime?.permission ?? 'unknown';
  const platform = runtime?.platform ?? 'unsupported';
  const permissionColor = permission === 'granted' || permission === 'not_required'
    ? 'success'
    : permission === 'denied'
      ? 'error'
      : 'default';
  const permissionHintKey = platform === 'macos'
    ? permission === 'granted'
      ? 'settings.selectionToolbar.permissionGrantedHint'
      : permission === 'denied'
        ? 'settings.selectionToolbar.permissionDeniedHint'
        : null
    : platform === 'windows'
      ? 'settings.selectionToolbar.permissionWindowsHint'
      : platform === 'linux'
        ? 'settings.selectionToolbar.permissionLinuxHint'
        : null;
  const runtimeColor = runtime?.state === 'running'
    ? 'success'
    : runtime?.state === 'error'
      ? 'error'
      : runtime?.state === 'permission_required' || runtime?.state === 'unavailable'
        ? 'warning'
        : runtime?.state === 'starting'
          ? 'processing'
          : 'default';

  const openPermissionSettings = () => {
    void invoke<SelectionToolbarPermissionSettingsOutcome>(
      'selection_toolbar_open_permission_settings',
    )
      .then((outcome) => {
        setManualPermissionPath(
          outcome.kind === 'manual_add_required'
            ? outcome.executable_path
            : null,
        );
      })
      .catch((error) => message.error(String(error)));
  };

  const startPermissionGuide = () => {
    setPermissionGuideOpen(true);
    void invoke('selection_toolbar_request_permission')
      .then(openPermissionSettings)
      .catch((error) => message.error(String(error)));
  };

  const replaceTool = (nextTool: SelectionToolbarTool) => {
    persist({
      ...settings,
      tools: settings.tools.map((tool) => toolId(tool) === toolId(nextTool) ? nextTool : tool),
    });
    setEditing(null);
  };

  const addTool = () => {
    const id = crypto.randomUUID();
    setEditing({
      kind: 'custom_ai',
      id,
      name: t('settings.selectionToolbar.newTool'),
      icon: 'wand-sparkles',
      enabled: true,
      ai: {
        prompt: '{selection}',
        provider_id: null,
        model_id: null,
        temperature: null,
        top_p: null,
        max_tokens: null,
      },
    });
  };

  const saveEditor = (tool: SelectionToolbarTool) => {
    const exists = settings.tools.some((item) => toolId(item) === toolId(tool));
    if (exists) replaceTool(tool);
    else {
      persist({ ...settings, tools: [...settings.tools, tool] });
      setEditing(null);
    }
  };

  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || active.id === over.id) return;
    const from = settings.tools.findIndex((tool) => toolId(tool) === active.id);
    const to = settings.tools.findIndex((tool) => toolId(tool) === over.id);
    if (from < 0 || to < 0) return;
    const tools = [...settings.tools];
    const [moved] = tools.splice(from, 1);
    tools.splice(to, 0, moved);
    persist({ ...settings, tools });
  };

  return (
    <div
      className="p-6 pb-12"
      data-testid="selection-toolbar-settings"
      style={{ boxSizing: 'border-box', width: '100%' }}
    >
      <SettingsGroup title={t('settings.selectionToolbar.title')}>
        <div style={{ alignItems: 'center', display: 'flex', justifyContent: 'space-between', padding: '8px 0' }}>
          <div>
            <div>{t('settings.selectionToolbar.enabled')}</div>
            <div style={{ color: 'var(--text-color-secondary)', fontSize: 12 }}>
              {t('settings.selectionToolbar.enabledHint')}
            </div>
            <div style={{ color: 'var(--text-color-secondary)', fontSize: 12 }}>
              {t('settings.selectionToolbar.supportedAppsHint')}
            </div>
          </div>
          <Switch
            aria-label={t('settings.selectionToolbar.enabled')}
            checked={settings.enabled}
            onChange={(enabled) => persist({ ...settings, enabled })}
          />
        </div>
        <Divider style={{ margin: 0 }} />
        <div style={{ alignItems: 'center', display: 'flex', justifyContent: 'space-between', padding: '12px 0' }}>
          <span>{t('settings.selectionToolbar.themeFollow')}</span>
          <Switch
            aria-label={t('settings.selectionToolbar.themeFollow')}
            checked={settings.theme_follow}
            onChange={(theme_follow) => persist({ ...settings, theme_follow })}
          />
        </div>
        <Divider style={{ margin: 0 }} />
        <div style={{ alignItems: 'center', display: 'flex', gap: 12, justifyContent: 'space-between', padding: '12px 0 4px' }}>
          <div style={{ minWidth: 0 }}>
            <div>{t('settings.selectionToolbar.translateTargetLanguage')}</div>
            <div style={{ color: 'var(--text-color-secondary)', fontSize: 12 }}>
              {t('settings.selectionToolbar.translateTargetHint')}
            </div>
          </div>
          <Select<string, { value: string; label: string; english: string }>
            aria-label={t('settings.selectionToolbar.translateTargetLanguage')}
            filterOption={(input, option) => {
              const query = input.trim().toLowerCase();
              if (!query || !option) return true;
              return option.value.toLowerCase().includes(query)
                || option.label.toLowerCase().includes(query)
                || option.english.toLowerCase().includes(query);
            }}
            options={[
              {
                value: 'follow',
                label: t('settings.selectionToolbar.translateFollowApp'),
                english: 'follow application language',
              },
              ...SELECTION_TRANSLATE_LANGUAGES.map((language: SelectionTranslateLanguage) => ({
                value: language.code,
                label: language.native,
                english: language.english,
              })),
            ]}
            showSearch
            style={{ flex: '0 0 auto', width: 200 }}
            value={settings.translate_target_language ?? 'follow'}
            onChange={(value) => persist({
              ...settings,
              translate_target_language: value === 'follow' ? null : value,
            })}
          />
        </div>
      </SettingsGroup>
      <SettingsGroup title={t('settings.selectionToolbar.permissionGroupTitle')}>
        <div style={{ padding: '8px 0 4px' }}>
          <div style={{ alignItems: 'center', display: 'flex', justifyContent: 'space-between', gap: 12 }}>
            <div style={{ minWidth: 0 }}>
              <div style={{ alignItems: 'center', display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                <span>{t('settings.selectionToolbar.permissionTitle')}</span>
                <Tag color={permissionColor} style={{ marginInlineEnd: 0 }}>
                  {t(`settings.selectionToolbar.permission.${permission}`)}
                </Tag>
              </div>
              <div style={{ color: token.colorTextDescription, fontSize: 12, marginTop: 4 }}>
                {t(`settings.selectionToolbar.platformMechanism.${platform}`)}
              </div>
            </div>
            {platform === 'macos' && permission !== 'granted' && (
              <div style={{ alignItems: 'center', display: 'flex', flex: '0 0 auto', gap: 4 }}>
                <Button size="small" type="primary" onClick={startPermissionGuide}>
                  {t('settings.selectionToolbar.requestPermission')}
                </Button>
                <Button size="small" type="link" onClick={openPermissionSettings}>
                  {t('settings.selectionToolbar.openPermission')}
                </Button>
              </div>
            )}
          </div>
          {permissionHintKey && (
            <div style={{ color: token.colorTextDescription, fontSize: 12, marginTop: 6 }}>
              {t(permissionHintKey)}
            </div>
          )}
          {runtime && (
            <div style={{ fontSize: 12, marginTop: 10 }}>
              <div style={{ alignItems: 'center', display: 'flex', gap: 4 }}>
                <span>{t('settings.selectionToolbar.runtimeTitle')}</span>
                <Tag color={runtimeColor}>
                  {t(`settings.selectionToolbar.status.${runtime.state}`)}
                </Tag>
                {(runtime.state === 'permission_required'
                  || runtime.state === 'unavailable'
                  || runtime.state === 'error') && (
                  <Button
                    size="small"
                    type="link"
                    onClick={() => void invoke('selection_toolbar_retry_monitoring')
                      .then(refreshRuntime)
                      .catch((error) => message.error(String(error)))}
                  >
                    {t('settings.selectionToolbar.retry')}
                  </Button>
                )}
              </div>
              {runtime.last_error?.message && (
                <div style={{ color: token.colorTextDescription, marginTop: 4 }}>
                  {runtime.last_error.message}
                </div>
              )}
            </div>
          )}
          {manualPermissionPath && (
            <div
              role="alert"
              style={{
                background: token.colorWarningBg,
                border: `1px solid ${token.colorWarningBorder}`,
                borderRadius: token.borderRadius,
                color: token.colorText,
                fontSize: 12,
                marginTop: 10,
                padding: '8px 10px',
                wordBreak: 'break-all',
              }}
            >
              {t('settings.selectionToolbar.developmentPermissionHint', {
                path: manualPermissionPath,
              })}
            </div>
          )}
        </div>
      </SettingsGroup>
      <Modal
        footer={permission === 'granted'
          ? [
              <Button key="done" type="primary" onClick={() => setPermissionGuideOpen(false)}>
                {t('settings.selectionToolbar.guideDone')}
              </Button>,
            ]
          : [
              <Button key="close" onClick={() => setPermissionGuideOpen(false)}>
                {t('common.close')}
              </Button>,
              <Button key="open" type="primary" onClick={openPermissionSettings}>
                {t('settings.selectionToolbar.openPermission')}
              </Button>,
            ]}
        onCancel={() => setPermissionGuideOpen(false)}
        open={permissionGuideOpen}
        title={t('settings.selectionToolbar.guideTitle')}
      >
        {permission === 'granted' ? (
          <Alert
            message={t('settings.selectionToolbar.guideGranted')}
            showIcon
            type="success"
          />
        ) : (
          <>
            <div style={{ color: token.colorTextDescription, marginBottom: 16 }}>
              {t('settings.selectionToolbar.guideIntro')}
            </div>
            <Steps
              current={0}
              direction="vertical"
              items={[
                { title: t('settings.selectionToolbar.guideStepOpen') },
                { title: t('settings.selectionToolbar.guideStepEnable') },
                { title: t('settings.selectionToolbar.guideStepReturn') },
              ]}
              size="small"
            />
          </>
        )}
        <div style={{ alignItems: 'center', display: 'flex', gap: 8, marginTop: 18 }}>
          <span>{t('settings.selectionToolbar.permissionTitle')}</span>
          <Tag color={permissionColor} style={{ marginInlineEnd: 0 }}>
            {t(`settings.selectionToolbar.permission.${permission}`)}
          </Tag>
        </div>
      </Modal>

      <SettingsGroup
        extra={<Button icon={<Plus size={14} />} size="small" onClick={addTool}>{t('settings.selectionToolbar.addTool')}</Button>}
        title={t('settings.selectionToolbar.toolsTitle')}
      >
        <DndContext collisionDetection={closestCenter} sensors={sensors} onDragEnd={handleDragEnd}>
          <SortableContext items={ids} strategy={verticalListSortingStrategy}>
            {settings.tools.map((tool, index) => (
              <div key={toolId(tool)}>
                {index > 0 && <Divider style={{ margin: 0 }} />}
                <SortableToolRow
                  tool={tool}
                  onDelete={() => persist({ ...settings, tools: settings.tools.filter((item) => toolId(item) !== toolId(tool)) })}
                  onEdit={() => setEditing(tool)}
                  onReset={() => {
                    const defaultTool = createDefaultSelectionToolbarSettings().tools.find((item) => toolId(item) === toolId(tool));
                    if (defaultTool) replaceTool(defaultTool);
                  }}
                  onToggle={(enabled) => replaceTool({ ...tool, enabled })}
                />
              </div>
            ))}
          </SortableContext>
        </DndContext>
      </SettingsGroup>
      <ToolEditor tool={editing} onClose={() => setEditing(null)} onSave={saveEditor} />
    </div>
  );
}
