import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  App,
  Avatar,
  Button,
  Dropdown,
  Empty,
  Input,
  Spin,
  Tooltip,
  theme,
} from 'antd';
import Conversations from '@ant-design/x/es/conversations';
import type { ConversationItemType } from '@ant-design/x/es/conversations/interface';
import {
  FolderOpen,
  FolderPlus,
  GripVertical,
  Loader,
  MessageSquarePlus,
  Search,
  Settings,
  Trash2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  DragOverlay,
  useDraggable,
  useDroppable,
  type DragEndEvent,
  type DragStartEvent,
  type DragOverEvent,
} from '@dnd-kit/core';
import { useAcpStore } from '@/stores/acpStore';
import { useUIStore } from '@/stores';
import { AcpAgentIcon } from '@/lib/acpAgentIcon';
import { getAcpProjectIcon } from '@/lib/acpProjectIcon';
import { useResolvedAvatarSrc } from '@/hooks/useResolvedAvatarSrc';
import type { AcpProject, AcpThread } from '@/types/acp';
import type { AvatarType } from '@/stores/userProfileStore';
import { AcpProjectSettingsModal } from './AcpProjectSettingsModal';

/** Same title ellipsis shell as ChatSidebar ConversationTitleText */
function ThreadTitleText({ title, className = '' }: { title: string; className?: string }) {
  const mergedClassName = ['aqbot-chat-conversation-title', className].filter(Boolean).join(' ');
  return (
    <span
      className={mergedClassName}
      title={title}
      style={{
        display: 'block',
        minWidth: 0,
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
      }}
    >
      {title}
    </span>
  );
}

function ProjectIcon({ projectId, size = 13 }: { projectId: string; size?: number }) {
  const { token } = theme.useToken();
  const icon = getAcpProjectIcon(projectId);
  const resolvedSrc = useResolvedAvatarSrc(
    (icon?.type as AvatarType) ?? 'icon',
    icon?.value ?? '',
  );

  if (icon?.type === 'emoji' && icon.value) {
    return (
      <Avatar
        size={size + 4}
        style={{
          fontSize: Math.max(10, size * 0.9),
          backgroundColor: token.colorPrimaryBg,
          flexShrink: 0,
          lineHeight: 1,
        }}
      >
        {icon.value}
      </Avatar>
    );
  }

  if (icon && (icon.type === 'url' || icon.type === 'file' || icon.type === 'model_icon') && icon.value) {
    const src =
      icon.type === 'file'
        ? (resolvedSrc ?? (icon.value.startsWith('data:') ? icon.value : undefined))
        : icon.type === 'model_icon'
          ? undefined
          : icon.value;
    if (src) {
      return <Avatar size={size + 4} src={src} style={{ flexShrink: 0 }} />;
    }
    if (icon.type === 'model_icon') {
      // model icon value is usually a lobe icon id rendered elsewhere; fall back to folder
    }
  }

  return <FolderOpen size={size} style={{ flexShrink: 0 }} />;
}

/**
 * Project row = ChatSidebar SortableCategoryLabel 1:1
 * grip + icon + name, context menu, dnd-kit.
 */
function SortableProjectLabel({
  project,
  menuActionRef,
  onNewThread,
  onSettings,
  onDelete,
  newThreadLabel,
  settingsLabel,
  deleteLabel,
}: {
  project: AcpProject;
  menuActionRef: React.MutableRefObject<boolean>;
  onNewThread: () => void;
  onSettings: () => void;
  onDelete: () => void;
  newThreadLabel: string;
  settingsLabel: string;
  deleteLabel: string;
}) {
  const { attributes, listeners, setNodeRef: setDragRef, isDragging } = useDraggable({
    id: project.id,
  });
  const { setNodeRef: setDropRef } = useDroppable({ id: project.id });
  const mergedRef = useCallback(
    (node: HTMLDivElement | null) => {
      setDragRef(node);
      setDropRef(node);
    },
    [setDragRef, setDropRef],
  );

  return (
    <Dropdown
      trigger={['contextMenu']}
      menu={{
        items: [
          { key: 'new', label: newThreadLabel, icon: <MessageSquarePlus size={14} /> },
          { key: 'settings', label: settingsLabel, icon: <Settings size={14} /> },
          { key: 'delete', label: deleteLabel, icon: <Trash2 size={14} />, danger: true },
        ],
        onClick: ({ key, domEvent }) => {
          domEvent.stopPropagation();
          menuActionRef.current = true;
          setTimeout(() => {
            menuActionRef.current = false;
          }, 100);
          if (key === 'new') onNewThread();
          else if (key === 'settings') onSettings();
          else if (key === 'delete') onDelete();
        },
      }}
    >
      <div
        ref={mergedRef}
        className="flex items-center gap-1"
        style={{ opacity: isDragging ? 0.3 : 1, cursor: 'pointer', userSelect: 'none', flex: 1 }}
        {...attributes}
        {...listeners}
        title={project.root_path}
      >
        <GripVertical size={12} style={{ opacity: 0.4, cursor: 'grab', flexShrink: 0 }} />
        <ProjectIcon projectId={project.id} size={13} />
        <span className="truncate">{project.name}</span>
      </div>
    </Dropdown>
  );
}

function ThreadListIcon({
  agentId,
  agentName,
  agentIcon,
  isStreaming,
  size = 20,
}: {
  agentId: string;
  agentName: string;
  agentIcon?: string | null;
  isStreaming: boolean;
  size?: number;
}) {
  const { token } = theme.useToken();
  let icon: React.ReactNode = (
    <AcpAgentIcon agentId={agentId} agentName={agentName} icon={agentIcon} size={size} />
  );

  if (isStreaming) {
    icon = (
      <span style={{ position: 'relative', display: 'inline-flex' }}>
        {icon}
        <Loader
          size={Math.max(8, Math.round(size * 0.5))}
          style={{
            position: 'absolute',
            bottom: -3,
            right: -3,
            color: token.colorPrimary,
            background: token.colorBgContainer,
            borderRadius: '50%',
            animation: 'spin 1s linear infinite',
          }}
        />
      </span>
    );
  }

  return icon;
}

/**
 * Agent sidebar — structural 1:1 of ChatSidebar:
 * toolbar / Conversations list / category groups / circular avatar icons / same CSS.
 * Projects = categories, Threads = conversations.
 * No per-project "new chat" button under the group.
 */
export function AcpSidebar() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { modal } = App.useApp();
  const setActivePage = useUIStore((s) => s.setActivePage);
  const setSettingsSection = useUIStore((s) => s.setSettingsSection);

  const projects = useAcpStore((s) => s.projects);
  const allThreads = useAcpStore((s) => s.allThreads);
  const threads = useAcpStore((s) => s.threads);
  const activeProjectId = useAcpStore((s) => s.activeProjectId);
  const activeThreadId = useAcpStore((s) => s.activeThreadId);
  const runningByThread = useAcpStore((s) => s.runningByThread);
  const loadProjects = useAcpStore((s) => s.loadProjects);
  const loadAllThreads = useAcpStore((s) => s.loadAllThreads);
  const setProjectsOrder = useAcpStore((s) => s.setProjectsOrder);
  const reorderProjects = useAcpStore((s) => s.reorderProjects);
  const createProject = useAcpStore((s) => s.createProject);
  const deleteProject = useAcpStore((s) => s.deleteProject);
  const selectProject = useAcpStore((s) => s.selectProject);
  const selectThread = useAcpStore((s) => s.selectThread);
  const deleteThread = useAcpStore((s) => s.deleteThread);
  const enabledAgents = useAcpStore((s) => s.enabledAgents);
  const configReady = useAcpStore((s) => s.configReady);
  const projectsReady = useAcpStore((s) => s.projectsReady);
  const agents = enabledAgents();
  // Cold start: config not fetched yet and no cached agents → loading, not "未配置"
  const showAgentsLoading = !configReady && agents.length === 0;
  const showProjectsLoading = !showAgentsLoading && agents.length > 0 && !projectsReady && projects.length === 0;

  const [query, setQuery] = useState('');
  const [searchOpen, setSearchOpen] = useState(false);
  /** expandedKeys use `proj:{id}` like chat uses `cat:{id}` */
  const [expandedKeys, setExpandedKeys] = useState<string[]>([]);
  const [settingsProject, setSettingsProject] = useState<AcpProject | null>(null);
  const menuActionRef = useRef(false);
  const listScrollRef = useRef<HTMLDivElement>(null);

  const dndSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );
  const [activeDragProjectId, setActiveDragProjectId] = useState<string | null>(null);
  const dragInitialOrderRef = useRef<string[]>([]);

  useEffect(() => {
    void loadProjects();
    void loadAllThreads();
  }, [loadProjects, loadAllThreads]);

  // Auto-expand active project (same as chat auto-expand category)
  useEffect(() => {
    if (!activeProjectId) return;
    const key = `proj:${activeProjectId}`;
    setExpandedKeys((prev) => (prev.includes(key) ? prev : [...prev, key]));
  }, [activeProjectId]);

  const agentName = useCallback(
    (id: string) => agents.find((a) => a.id === id)?.name ?? id,
    [agents],
  );

  const agentIcon = useCallback(
    (id: string) => agents.find((a) => a.id === id)?.icon ?? null,
    [agents],
  );

  const threadsForProject = useCallback(
    (projectId: string): AcpThread[] => {
      if (projectId === activeProjectId && threads.length > 0) return threads;
      return allThreads.filter((th) => th.project_id === projectId);
    },
    [activeProjectId, threads, allThreads],
  );

  const filteredProjects = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return projects;
    return projects.filter((p) => {
      if (p.name.toLowerCase().includes(q) || p.root_path.toLowerCase().includes(q)) return true;
      return threadsForProject(p.id).some(
        (th) =>
          th.title.toLowerCase().includes(q)
          || th.agent_id.toLowerCase().includes(q)
          || agentName(th.agent_id).toLowerCase().includes(q),
      );
    });
  }, [projects, query, threadsForProject, agentName]);

  const projectById = useMemo(
    () => new Map(projects.map((p) => [p.id, p])),
    [projects],
  );

  const handleProjectDragStart = useCallback(
    (event: DragStartEvent) => {
      setActiveDragProjectId(String(event.active.id));
      dragInitialOrderRef.current = projects.map((p) => p.id);
    },
    [projects],
  );

  const handleProjectDragOver = useCallback(
    (event: DragOverEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const ids = projects.map((p) => p.id);
      const oldIndex = ids.indexOf(String(active.id));
      const newIndex = ids.indexOf(String(over.id));
      if (oldIndex === -1 || newIndex === -1 || oldIndex === newIndex) return;
      const newIds = [...ids];
      newIds.splice(oldIndex, 1);
      newIds.splice(newIndex, 0, String(active.id));
      setProjectsOrder(
        newIds
          .map((id, i) => {
            const p = projects.find((x) => x.id === id);
            return p ? { ...p, sort_order: i } : null;
          })
          .filter(Boolean) as AcpProject[],
      );
    },
    [projects, setProjectsOrder],
  );

  const handleProjectDragEnd = useCallback(
    (_event: DragEndEvent) => {
      setActiveDragProjectId(null);
      const ids = useAcpStore.getState().projects.map((p) => p.id);
      void reorderProjects(ids);
    },
    [reorderProjects],
  );

  const handleProjectDragCancel = useCallback(() => {
    setActiveDragProjectId(null);
    const initial = dragInitialOrderRef.current;
    if (initial.length === 0) return;
    const current = useAcpStore.getState().projects;
    setProjectsOrder(
      initial
        .map((id, i) => {
          const p = current.find((x) => x.id === id);
          return p ? { ...p, sort_order: i } : null;
        })
        .filter(Boolean) as AcpProject[],
    );
  }, [setProjectsOrder]);

  const handleGroupExpand = useCallback(
    (keys: string[]) => {
      if (menuActionRef.current) return;
      // Newly expanded project → select it (right pane shows input)
      const newly = keys.find((k) => !expandedKeys.includes(k) && k.startsWith('proj:'));
      setExpandedKeys(keys);
      if (newly) {
        void selectProject(newly.slice(5));
      }
    },
    [expandedKeys, selectProject],
  );

  const handleAddProject = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    const rootPath = selected;
    const name = rootPath.split(/[/\\]/).filter(Boolean).pop() || 'Project';
    const project = await createProject(name, rootPath);
    setExpandedKeys((prev) => [...prev, `proj:${project.id}`]);
    await selectProject(project.id);
    // Open settings so user can set icon / rename after import
    setSettingsProject(project);
  };

  /** Clear thread, keep project — right pane shows project empty + input */
  const handleNewThreadInProject = useCallback(async (projectId: string) => {
    if (agents.length === 0) {
      setSettingsSection('acpAgents');
      setActivePage('settings');
      return;
    }
    setExpandedKeys((prev) => {
      const key = `proj:${projectId}`;
      return prev.includes(key) ? prev : [...prev, key];
    });
    await selectProject(projectId);
  }, [agents.length, selectProject, setActivePage, setSettingsSection]);

  // Shortcut: new thread in current project (parity with chat new conversation)
  useEffect(() => {
    const onNew = () => {
      if (activeProjectId) void handleNewThreadInProject(activeProjectId);
    };
    const onCloseThread = () => {
      void selectThread(null);
    };
    const onOpenSearch = () => {
      setSearchOpen(true);
      requestAnimationFrame(() => {
        document.querySelector<HTMLInputElement>('.chat-sidebar-search input')?.focus();
      });
    };
    window.addEventListener('aqbot:new-agent-thread', onNew);
    window.addEventListener('aqbot:close-agent-thread', onCloseThread);
    window.addEventListener('aqbot:open-agent-search', onOpenSearch);
    return () => {
      window.removeEventListener('aqbot:new-agent-thread', onNew);
      window.removeEventListener('aqbot:close-agent-thread', onCloseThread);
      window.removeEventListener('aqbot:open-agent-search', onOpenSearch);
    };
  }, [activeProjectId, handleNewThreadInProject, selectThread]);

  const handleDeleteProject = useCallback(
    (project: AcpProject) => {
      modal.confirm({
        title: t('agentPage.deleteProject'),
        content: project.name,
        okButtonProps: { danger: true },
        okText: t('common.confirm'),
        cancelText: t('common.cancel'),
        onOk: async () => {
          await deleteProject(project.id);
        },
      });
    },
    [deleteProject, modal, t],
  );

  const openSettings = () => {
    setSettingsSection('acpAgents');
    setActivePage('settings');
  };

  // ── Conversations items — same shape as ChatSidebar getConversationItem ──
  const conversationItems: ConversationItemType[] = useMemo(() => {
    const items: ConversationItemType[] = [];
    const q = query.trim().toLowerCase();

    for (const project of filteredProjects) {
      const group = `proj:${project.id}`;
      const expanded = expandedKeys.includes(group);
      let projectThreads = threadsForProject(project.id);
      if (q) {
        projectThreads = projectThreads.filter(
          (th) =>
            th.title.toLowerCase().includes(q)
            || th.agent_id.toLowerCase().includes(q)
            || agentName(th.agent_id).toLowerCase().includes(q),
        );
      }

      if (!expanded) {
        items.push({
          key: `__collapsed_${group}`,
          group,
          label: null,
          disabled: true,
          style: { display: 'none' },
        });
        continue;
      }

      if (projectThreads.length === 0) {
        items.push({
          key: `__empty_${project.id}`,
          group,
          label: (
            <span style={{ color: token.colorTextQuaternary, fontSize: 12, fontStyle: 'italic' }}>
              {t('agentPage.emptyProjectThreads', '暂无对话')}
            </span>
          ),
          icon: null,
          disabled: true,
          style: { pointerEvents: 'none', minHeight: 28, opacity: 0.6 },
        });
        continue;
      }

      for (const th of projectThreads) {
        const running = !!runningByThread[th.id];
        items.push({
          key: th.id,
          group,
          // Avatar + streaming loader badge (chat ConversationIcon parity)
          icon: (
            <ThreadListIcon
              agentId={th.agent_id}
              agentName={agentName(th.agent_id)}
              agentIcon={agentIcon(th.agent_id)}
              isStreaming={running}
              size={20}
            />
          ),
          label: (
            <span className="aqbot-chat-conversation-label">
              <ThreadTitleText title={th.title} className="flex-1" />
            </span>
          ),
          'data-conv-id': th.id,
        } as ConversationItemType);
      }
    }
    return items;
  }, [
    filteredProjects,
    expandedKeys,
    threadsForProject,
    query,
    agentName,
    agentIcon,
    runningByThread,
    token.colorTextQuaternary,
    t,
  ]);

  const renderGroupLabel = useCallback(
    (group: string) => {
      if (!group.startsWith('proj:')) return group;
      const projectId = group.slice(5);
      const project = projectById.get(projectId);
      if (!project) return group;
      return (
        <SortableProjectLabel
          project={project}
          menuActionRef={menuActionRef}
          newThreadLabel={t('agentPage.newThread')}
          settingsLabel={t('agentPage.projectSettings', '项目设置')}
          deleteLabel={t('agentPage.deleteProject')}
          onNewThread={() => void handleNewThreadInProject(projectId)}
          onSettings={() => setSettingsProject(project)}
          onDelete={() => handleDeleteProject(project)}
        />
      );
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [projectById, t, handleDeleteProject, handleNewThreadInProject],
  );

  const groupableConfig = useMemo(
    () => ({
      label: (group: string) => renderGroupLabel(group),
      collapsible: (group: string) => group.startsWith('proj:'),
      expandedKeys,
      onExpand: handleGroupExpand,
    }),
    [expandedKeys, handleGroupExpand, renderGroupLabel],
  );

  const handleActiveChange = useCallback(
    (key: string) => {
      if (key.startsWith('__')) return;
      const thread =
        allThreads.find((th) => th.id === key)
        ?? threads.find((th) => th.id === key);
      if (!thread) return;
      // selectThread also switches activeProjectId when needed (without clearing the thread)
      void selectThread(thread.id);
    },
    [allThreads, threads, selectThread],
  );

  const menuFactory = useCallback(
    (item: ConversationItemType) => {
      const id = String(item.key);
      if (id.startsWith('__')) return undefined;
      return {
        items: [
          {
            key: 'delete',
            danger: true,
            icon: <Trash2 size={14} />,
            label: t('agentPage.deleteThread'),
            onClick: () => {
              void deleteThread(id);
            },
          },
        ],
      };
    },
    [deleteThread, t],
  );

  // ── Layout: identical to ChatSidebar outer structure ────────────────
  return (
    <div className="flex flex-col h-full">
      {/* Toolbar — same padding / buttons row as ChatSidebar */}
      <div
        className="flex items-center justify-between"
        style={{
          padding: '8px 12px',
          borderBottom: '1px solid var(--border-color)',
        }}
      >
        <div className="flex items-center gap-1">
          <Tooltip title={t('chat.searchPlaceholder')}>
            <Button
              type="text"
              icon={<Search size={16} />}
              size="small"
              aria-label={t('chat.searchPlaceholder')}
              onClick={() => setSearchOpen((v) => !v)}
            />
          </Tooltip>
          <Tooltip title={t('agentPage.addProject')}>
            <Button
              type="text"
              icon={<FolderPlus size={16} />}
              size="small"
              aria-label={t('agentPage.addProject')}
              onClick={() => void handleAddProject()}
            />
          </Tooltip>
          <Tooltip title={t('agentPage.newThread')}>
            <Button
              type="text"
              icon={<MessageSquarePlus size={16} />}
              size="small"
              aria-label={t('agentPage.newThread')}
              disabled={!activeProjectId || agents.length === 0}
              onClick={() => activeProjectId && void handleNewThreadInProject(activeProjectId)}
            />
          </Tooltip>
        </div>
        <div className="flex items-center gap-1">
          <Tooltip title={t('settings.acpAgents.title')}>
            <Button
              type="text"
              icon={<Settings size={16} />}
              size="small"
              aria-label={t('settings.acpAgents.title')}
              onClick={openSettings}
            />
          </Tooltip>
        </div>
      </div>

      {searchOpen && (
        <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border-color)' }}>
          <Input
            className="chat-sidebar-search"
            allowClear
            size="small"
            prefix={<Search size={14} />}
            placeholder={t('chat.searchPlaceholder')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            autoFocus
          />
        </div>
      )}

      {/* List shell — NO extra padding (ChatSidebar has none; padding lives in Conversations) */}
      <div ref={listScrollRef} className="flex-1 overflow-y-auto">
        {showAgentsLoading || showProjectsLoading ? (
          <div className="flex items-center justify-center h-full">
            <Spin tip={t('agentPage.loading')} />
          </div>
        ) : agents.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('agentPage.noAgents')}
            >
              <Button type="primary" size="small" onClick={openSettings}>
                {t('agentPage.openSettings')}
              </Button>
            </Empty>
          </div>
        ) : filteredProjects.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('agentPage.noProjects')}
            >
              <Button type="primary" size="small" onClick={() => void handleAddProject()}>
                {t('agentPage.addProject')}
              </Button>
            </Empty>
          </div>
        ) : (
          <div>
            {/* Exact CSS overrides copied from ChatSidebar */}
            <style>{`
              .ant-conversations .ant-conversations-item-active {
                background-color: ${token.colorPrimaryBg} !important;
              }
              .ant-conversations .ant-conversations-item-active .ant-conversations-label {
                color: ${token.colorPrimary} !important;
              }
              .ant-conversations .ant-conversations-icon {
                display: inline-flex;
                align-items: center;
                justify-content: center;
                flex-shrink: 0;
                line-height: 0;
              }
              .ant-conversations .ant-conversations-label {
                min-width: 0;
                overflow: hidden;
                display: flex !important;
                align-items: center;
                margin-bottom: 0 !important;
                line-height: 1.25;
              }
              .ant-conversations .ant-conversations-menu {
                display: inline-flex;
                align-items: center;
                justify-content: center;
                flex-shrink: 0;
                line-height: 0;
              }
              .ant-conversations .ant-conversations-item > div:has(.ant-conversations-menu-icon) {
                display: inline-flex;
                align-items: center;
                justify-content: center;
                flex-shrink: 0;
                line-height: 0;
              }
              .ant-conversations .ant-conversations-menu-icon {
                display: inline-flex !important;
                align-items: center;
                justify-content: center;
                width: 22px;
                height: 22px;
                min-width: 22px;
                line-height: 1;
                font-size: 16px;
                flex-shrink: 0;
                box-sizing: border-box;
              }
              .aqbot-chat-conversation-label {
                display: flex;
                align-items: center;
                gap: 4px;
                min-width: 0;
                width: 100%;
                overflow: hidden;
              }
              .aqbot-chat-conversation-title {
                min-width: 0;
                overflow: hidden;
                text-overflow: ellipsis;
                white-space: nowrap;
                display: block;
                line-height: 1.25;
              }
              .ant-conversations .ant-conversations-group-label {
                flex: 1;
                overflow: hidden;
              }
              .aqbot-conversation-model-icon {
                flex-shrink: 0;
              }
              @keyframes spin {
                from { transform: rotate(0deg); }
                to { transform: rotate(360deg); }
              }
            `}</style>
            <DndContext
              sensors={dndSensors}
              collisionDetection={closestCenter}
              onDragStart={handleProjectDragStart}
              onDragOver={handleProjectDragOver}
              onDragEnd={handleProjectDragEnd}
              onDragCancel={handleProjectDragCancel}
            >
              <Conversations
                items={conversationItems}
                activeKey={activeThreadId ?? undefined}
                onActiveChange={handleActiveChange}
                groupable={groupableConfig}
                menu={menuFactory}
              />
              <DragOverlay>
                {activeDragProjectId
                  ? (() => {
                      const project = projectById.get(activeDragProjectId);
                      if (!project) return null;
                      return (
                        <div
                          className="flex items-center gap-1"
                          style={{ opacity: 0.8, cursor: 'grabbing', fontSize: 13 }}
                        >
                          <GripVertical size={12} style={{ opacity: 0.4 }} />
                          <ProjectIcon projectId={project.id} size={13} />
                          <span>{project.name}</span>
                        </div>
                      );
                    })()
                  : null}
              </DragOverlay>
            </DndContext>
          </div>
        )}
      </div>

      <AcpProjectSettingsModal
        open={!!settingsProject}
        project={settingsProject}
        onClose={() => setSettingsProject(null)}
      />
    </div>
  );
}
