import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  App,
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
  ChevronDown,
  ChevronRight,
  Copy,
  FolderOpen,
  FolderPlus,
  GripVertical,
  Loader,
  MessageSquarePlus,
  Pencil,
  Pin,
  PinOff,
  Plus,
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
import { DynamicLobeIcon } from '@/components/shared/DynamicLobeIcon';
import { useResolvedAvatarSrc } from '@/hooks/useResolvedAvatarSrc';
import type { AcpProject, AcpThread } from '@/types/acp';
import type { AvatarType } from '@/stores/userProfileStore';
import { AcpProjectSettingsModal } from './AcpProjectSettingsModal';

/** Platform-aware "Reveal in Finder / Explorer / file manager" label. */
function useRevealInFolderLabel(): string {
  const { t } = useTranslation();
  return useMemo(() => {
    if (typeof navigator === 'undefined') {
      return t('agentPage.showInFolder', '在文件夹中显示');
    }
    const platform = navigator.platform || '';
    const ua = navigator.userAgent || '';
    if (/Mac|iPhone|iPad|iPod/i.test(platform) || (/Mac OS/i.test(ua) && !/Windows|Linux|Android/i.test(ua))) {
      return t('agentPage.showInFinder', '在 Finder 中显示');
    }
    if (/Win/i.test(platform) || /Windows/i.test(ua)) {
      return t('agentPage.showInExplorer', '在资源管理器中显示');
    }
    return t('agentPage.showInFileManager', '在文件管理器中显示');
  }, [t]);
}

async function revealPathInFolder(path: string): Promise<void> {
  const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
  await revealItemInDir(path);
}

function isThreadPinned(thread: AcpThread): boolean {
  return !!thread.is_pinned;
}

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

/**
 * Project list icon. Sized to match conversation row avatars (~20px) so custom
 * emoji / file / url icons stay readable (the old 13px Avatar looked tiny).
 */
function ProjectIcon({ projectId, size = 20 }: { projectId: string; size?: number }) {
  const icon = getAcpProjectIcon(projectId);
  const resolvedSrc = useResolvedAvatarSrc(
    (icon?.type as AvatarType) ?? 'icon',
    icon?.value ?? '',
  );

  if (icon?.type === 'emoji' && icon.value) {
    return (
      <span
        style={{
          width: size,
          height: size,
          fontSize: Math.round(size * 0.72),
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          flexShrink: 0,
          lineHeight: 1,
        }}
        aria-hidden
      >
        {icon.value}
      </span>
    );
  }

  if (icon?.type === 'model_icon' && icon.value) {
    const iconId = icon.value.includes(':') ? icon.value.slice(icon.value.indexOf(':') + 1) : icon.value;
    return (
      <span
        style={{
          width: size,
          height: size,
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          flexShrink: 0,
          lineHeight: 0,
        }}
        aria-hidden
      >
        <DynamicLobeIcon iconId={iconId} size={size} type="color" />
      </span>
    );
  }

  if (icon && (icon.type === 'url' || icon.type === 'file') && icon.value) {
    const src =
      icon.type === 'file'
        ? (resolvedSrc ?? (icon.value.startsWith('data:') ? icon.value : undefined))
        : icon.value;
    if (src) {
      return (
        <img
          src={src}
          alt=""
          style={{
            width: size,
            height: size,
            borderRadius: 4,
            objectFit: 'cover',
            flexShrink: 0,
            display: 'block',
          }}
        />
      );
    }
  }

  return <FolderOpen size={Math.max(14, size - 2)} style={{ flexShrink: 0 }} />;
}

/**
 * Project row = ChatSidebar SortableCategoryLabel 1:1
 * grip + icon + name, context menu, dnd-kit.
 */
function SortableProjectLabel({
  project,
  menuActionRef,
  onSelect,
  onNewThread,
  onSettings,
  onReveal,
  onDelete,
  newThreadLabel,
  settingsLabel,
  revealLabel,
  deleteLabel,
}: {
  project: AcpProject;
  menuActionRef: React.MutableRefObject<boolean>;
  /** Clicking the label (not the chevron) selects the project → new conversation pane. */
  onSelect: () => void;
  onNewThread: () => void;
  onSettings: () => void;
  onReveal: () => void;
  onDelete: () => void;
  newThreadLabel: string;
  settingsLabel: string;
  revealLabel: string;
  deleteLabel: string;
}) {
  const { attributes, listeners, setNodeRef: setDragRef, isDragging } = useDraggable({
    id: `proj:${project.id}`,
    data: { type: 'project', projectId: project.id },
  });
  const { setNodeRef: setDropRef } = useDroppable({
    id: `proj:${project.id}`,
    data: { type: 'project', projectId: project.id },
  });
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
          { key: 'reveal', label: revealLabel, icon: <FolderOpen size={14} /> },
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
          else if (key === 'reveal') onReveal();
          else if (key === 'delete') onDelete();
        },
      }}
    >
      <div
        ref={mergedRef}
        className="flex items-center gap-1.5"
        style={{ opacity: isDragging ? 0.3 : 1, cursor: 'pointer', userSelect: 'none', flex: 1 }}
        {...attributes}
        {...listeners}
        title={project.root_path}
        onClick={(event) => {
          // Own toggle+select (stop parent so we don't double-toggle).
          event.stopPropagation();
          onSelect();
        }}
      >
        <GripVertical size={12} style={{ opacity: 0.4, cursor: 'grab', flexShrink: 0 }} />
        <ProjectIcon projectId={project.id} size={20} />
        <span className="truncate">{project.name}</span>
      </div>
    </Dropdown>
  );
}

/** Thread row title with drag handle for in-project reordering. Right-click is handled by the list shell. */
function SortableThreadLabel({
  thread,
  title,
}: {
  thread: AcpThread;
  title: string;
}) {
  const { attributes, listeners, setNodeRef: setDragRef, isDragging } = useDraggable({
    id: `thread:${thread.id}`,
    data: { type: 'thread', threadId: thread.id, projectId: thread.project_id },
  });
  const { setNodeRef: setDropRef } = useDroppable({
    id: `thread:${thread.id}`,
    data: { type: 'thread', threadId: thread.id, projectId: thread.project_id },
  });
  const mergedRef = useCallback(
    (node: HTMLDivElement | null) => {
      setDragRef(node);
      setDropRef(node);
    },
    [setDragRef, setDropRef],
  );

  return (
    <span
      ref={mergedRef}
      className="aqbot-chat-conversation-label"
      data-conv-id={thread.id}
      style={{ opacity: isDragging ? 0.35 : 1, cursor: 'grab', width: '100%' }}
      {...attributes}
      {...listeners}
    >
      {isThreadPinned(thread) ? (
        <Pin size={12} style={{ opacity: 0.55, flexShrink: 0 }} aria-hidden />
      ) : null}
      <ThreadTitleText title={title} className="flex-1" />
    </span>
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
  const { modal, message: messageApi } = App.useApp();
  const setActivePage = useUIStore((s) => s.setActivePage);
  const setSettingsSection = useUIStore((s) => s.setSettingsSection);
  const revealLabel = useRevealInFolderLabel();

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
  const setThreadsOrder = useAcpStore((s) => s.setThreadsOrder);
  const reorderThreads = useAcpStore((s) => s.reorderThreads);
  const createProject = useAcpStore((s) => s.createProject);
  const deleteProject = useAcpStore((s) => s.deleteProject);
  const selectProject = useAcpStore((s) => s.selectProject);
  const selectThread = useAcpStore((s) => s.selectThread);
  const deleteThread = useAcpStore((s) => s.deleteThread);
  const renameThread = useAcpStore((s) => s.renameThread);
  const toggleThreadPin = useAcpStore((s) => s.toggleThreadPin);
  const duplicateThread = useAcpStore((s) => s.duplicateThread);
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
  const [expandedSections, setExpandedSections] = useState({
    projects: true,
    recent: true,
  });
  const [settingsProject, setSettingsProject] = useState<AcpProject | null>(null);
  const [rightClickedThreadId, setRightClickedThreadId] = useState<string | null>(null);
  const menuActionRef = useRef(false);
  /**
   * When the user collapses a project via label click, selectProject may change
   * activeProjectId and the auto-expand effect would immediately re-open it.
   * This ref blocks that one-shot re-expand.
   */
  const skipAutoExpandProjectIdRef = useRef<string | null>(null);
  const listScrollRef = useRef<HTMLDivElement>(null);

  const dndSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );
  const [activeDragProjectId, setActiveDragProjectId] = useState<string | null>(null);
  const [activeDragThread, setActiveDragThread] = useState<AcpThread | null>(null);
  const dragInitialProjectOrderRef = useRef<string[]>([]);
  const dragInitialThreadOrderRef = useRef<{ projectId: string; ids: string[] } | null>(null);

  useEffect(() => {
    void loadProjects();
    void loadAllThreads();
  }, [loadProjects, loadAllThreads]);

  // Auto-expand active project when selection changes (thread switch / import / etc.)
  // Skip once if the user intentionally collapsed that project on the same click that
  // also selected it — otherwise auto-expand would immediately undo the collapse.
  useEffect(() => {
    if (!activeProjectId) return;
    if (skipAutoExpandProjectIdRef.current === activeProjectId) {
      skipAutoExpandProjectIdRef.current = null;
      return;
    }
    // Drop stale skip when active project moved elsewhere
    skipAutoExpandProjectIdRef.current = null;
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

  const projectById = useMemo(
    () => new Map(projects.map((p) => [p.id, p])),
    [projects],
  );

  const userProjects = useMemo(
    () => projects.filter((project) => project.kind !== 'recent'),
    [projects],
  );

  const filteredProjects = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return userProjects;
    return userProjects.filter((p) => {
      if (p.name.toLowerCase().includes(q) || p.root_path.toLowerCase().includes(q)) return true;
      return threadsForProject(p.id).some(
        (th) =>
          th.title.toLowerCase().includes(q)
          || th.agent_id.toLowerCase().includes(q)
          || agentName(th.agent_id).toLowerCase().includes(q),
      );
    });
  }, [userProjects, query, threadsForProject, agentName]);

  const filteredRecentThreads = useMemo(() => {
    const recentProjectIds = new Set(
      projects.filter((project) => project.kind === 'recent').map((project) => project.id),
    );
    const q = query.trim().toLowerCase();
    return allThreads.filter((thread) => {
      if (!recentProjectIds.has(thread.project_id)) return false;
      if (!q) return true;
      return thread.title.toLowerCase().includes(q)
        || thread.agent_id.toLowerCase().includes(q)
        || agentName(thread.agent_id).toLowerCase().includes(q);
    });
  }, [projects, allThreads, query, agentName]);

  const parseDragId = useCallback((raw: string | number) => {
    const id = String(raw);
    if (id.startsWith('proj:')) return { type: 'project' as const, id: id.slice(5) };
    if (id.startsWith('thread:')) return { type: 'thread' as const, id: id.slice(7) };
    // Legacy bare project id (shouldn't happen after prefix migration)
    return { type: 'project' as const, id };
  }, []);

  const handleSidebarDragStart = useCallback(
    (event: DragStartEvent) => {
      const parsed = parseDragId(event.active.id);
      if (parsed.type === 'project') {
        setActiveDragProjectId(parsed.id);
        setActiveDragThread(null);
        dragInitialProjectOrderRef.current = userProjects.map((p) => p.id);
        dragInitialThreadOrderRef.current = null;
        return;
      }
      const thread =
        allThreads.find((th) => th.id === parsed.id)
        ?? threads.find((th) => th.id === parsed.id)
        ?? null;
      setActiveDragThread(thread);
      setActiveDragProjectId(null);
      if (thread) {
        const ids = threadsForProject(thread.project_id).map((th) => th.id);
        dragInitialThreadOrderRef.current = { projectId: thread.project_id, ids };
      }
      dragInitialProjectOrderRef.current = [];
    },
    [userProjects, allThreads, threads, threadsForProject, parseDragId],
  );

  const handleSidebarDragOver = useCallback(
    (event: DragOverEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const activeParsed = parseDragId(active.id);
      const overParsed = parseDragId(over.id);
      if (activeParsed.type !== overParsed.type) return;

      if (activeParsed.type === 'project') {
        const ids = userProjects.map((p) => p.id);
        const oldIndex = ids.indexOf(activeParsed.id);
        const newIndex = ids.indexOf(overParsed.id);
        if (oldIndex === -1 || newIndex === -1 || oldIndex === newIndex) return;
        const newIds = [...ids];
        newIds.splice(oldIndex, 1);
        newIds.splice(newIndex, 0, activeParsed.id);
        const reorderedProjects = newIds
            .map((id, i) => {
              const p = userProjects.find((x) => x.id === id);
              return p ? { ...p, sort_order: i } : null;
            })
            .filter(Boolean) as AcpProject[];
        setProjectsOrder([
          ...reorderedProjects,
          ...projects.filter((project) => project.kind === 'recent'),
        ]);
        return;
      }

      // Thread reorder — only within the same project
      const activeThread =
        allThreads.find((th) => th.id === activeParsed.id)
        ?? threads.find((th) => th.id === activeParsed.id);
      const overThread =
        allThreads.find((th) => th.id === overParsed.id)
        ?? threads.find((th) => th.id === overParsed.id);
      if (!activeThread || !overThread || activeThread.project_id !== overThread.project_id) return;
      // Keep pin group intact — only reorder within pinned or within unpinned
      if (isThreadPinned(activeThread) !== isThreadPinned(overThread)) return;

      const list = threadsForProject(activeThread.project_id);
      const ids = list.map((th) => th.id);
      const oldIndex = ids.indexOf(activeParsed.id);
      const newIndex = ids.indexOf(overParsed.id);
      if (oldIndex === -1 || newIndex === -1 || oldIndex === newIndex) return;
      const newIds = [...ids];
      newIds.splice(oldIndex, 1);
      newIds.splice(newIndex, 0, activeParsed.id);
      const ordered = newIds
        .map((id, i) => {
          const th = list.find((x) => x.id === id);
          return th ? { ...th, sort_order: i } : null;
        })
        .filter(Boolean) as AcpThread[];
      setThreadsOrder(activeThread.project_id, ordered);
    },
    [
      projects,
      userProjects,
      setProjectsOrder,
      allThreads,
      threads,
      threadsForProject,
      setThreadsOrder,
      parseDragId,
    ],
  );

  const handleSidebarDragEnd = useCallback(
    (_event: DragEndEvent) => {
      if (activeDragProjectId) {
        setActiveDragProjectId(null);
        const ids = useAcpStore.getState().projects
          .filter((project) => project.kind !== 'recent')
          .map((project) => project.id);
        void reorderProjects(ids);
      }
      if (activeDragThread) {
        const projectId = activeDragThread.project_id;
        setActiveDragThread(null);
        const ids = useAcpStore
          .getState()
          .allThreads
          .filter((th) => th.project_id === projectId)
          .map((th) => th.id);
        // Prefer in-order from threads list if active project
        const state = useAcpStore.getState();
        const orderedIds =
          state.activeProjectId === projectId && state.threads.length > 0
            ? state.threads.map((th) => th.id)
            : ids;
        void reorderThreads(projectId, orderedIds);
      }
      dragInitialProjectOrderRef.current = [];
      dragInitialThreadOrderRef.current = null;
    },
    [activeDragProjectId, activeDragThread, reorderProjects, reorderThreads],
  );

  const handleSidebarDragCancel = useCallback(() => {
    setActiveDragProjectId(null);
    setActiveDragThread(null);
    const initialProjects = dragInitialProjectOrderRef.current;
    if (initialProjects.length > 0) {
      const current = useAcpStore.getState().projects;
      setProjectsOrder(
        [
          ...initialProjects
          .map((id, i) => {
            const p = current.find((x) => x.id === id);
            return p ? { ...p, sort_order: i } : null;
          })
          .filter(Boolean) as AcpProject[],
          ...current.filter((project) => project.kind === 'recent'),
        ],
      );
    }
    const initialThreads = dragInitialThreadOrderRef.current;
    if (initialThreads) {
      const current = useAcpStore.getState().allThreads;
      const ordered = initialThreads.ids
        .map((id, i) => {
          const th = current.find((x) => x.id === id);
          return th ? { ...th, sort_order: i } : null;
        })
        .filter(Boolean) as AcpThread[];
      setThreadsOrder(initialThreads.projectId, ordered);
    }
    dragInitialProjectOrderRef.current = [];
    dragInitialThreadOrderRef.current = null;
  }, [setProjectsOrder, setThreadsOrder]);

  const handleRevealProject = useCallback(
    async (project: AcpProject) => {
      try {
        await revealPathInFolder(project.root_path);
      } catch (e) {
        messageApi.error(String(e));
      }
    },
    [messageApi],
  );

  const handleRenameThread = useCallback(
    (thread: AcpThread) => {
      let newTitle = thread.title;
      modal.confirm({
        title: t('chat.rename', '重命名'),
        mask: { enabled: true, blur: true },
        content: (
          <Input
            defaultValue={newTitle}
            onChange={(e) => {
              newTitle = e.target.value;
            }}
            onPressEnter={() => {
              // confirm dialog enter handled by OK button in most cases
            }}
            autoFocus
          />
        ),
        okText: t('common.confirm'),
        cancelText: t('common.cancel'),
        onOk: async () => {
          const trimmed = newTitle.trim();
          if (!trimmed || trimmed === thread.title) return;
          await renameThread(thread.id, trimmed);
        },
      });
    },
    [modal, renameThread, t],
  );

  const handleDuplicateThread = useCallback(
    (thread: AcpThread) => {
      void (async () => {
        try {
          const copy = await duplicateThread(
            thread.id,
            t('agentPage.copyThreadSuffix', ' (副本)'),
          );
          messageApi.success(t('agentPage.copyThreadSuccess', '已复制对话'));
          void selectThread(copy.id);
        } catch (e) {
          messageApi.error(String(e));
        }
      })();
    },
    [duplicateThread, messageApi, selectThread, t],
  );

  const handleDeleteThread = useCallback(
    (thread: AcpThread) => {
      modal.confirm({
        title: t('agentPage.deleteThread'),
        content: thread.title,
        okButtonProps: { danger: true },
        okText: t('common.confirm'),
        cancelText: t('common.cancel'),
        onOk: async () => {
          await deleteThread(thread.id);
        },
      });
    },
    [deleteThread, modal, t],
  );

  const handleGroupExpand = useCallback(
    (keys: string[]) => {
      if (menuActionRef.current) return;
      setExpandedKeys(keys);
      // Chevron-only expand/collapse: still select when newly expanded so the
      // right pane shows the project's new-conversation empty state.
      const newly = keys.find((k) => !expandedKeys.includes(k) && k.startsWith('proj:'));
      if (newly) {
        void selectProject(newly.slice(5));
      }
    },
    [expandedKeys, selectProject],
  );

  /**
   * Label click: toggle expand/collapse and select the project so the right
   * pane shows the new-conversation empty state.
   *
   * Collapsing while switching active project must not be undone by the
   * auto-expand effect (see skipAutoExpandProjectIdRef).
   */
  const handleSelectProject = useCallback(
    (projectId: string) => {
      const key = `proj:${projectId}`;
      setExpandedKeys((prev) => {
        const isOpen = prev.includes(key);
        if (isOpen) {
          // selectProject will set activeProjectId → auto-expand would re-open.
          // Block that only when the active id is about to change (or equal —
          // harmless if the effect doesn't re-run).
          skipAutoExpandProjectIdRef.current = projectId;
          return prev.filter((k) => k !== key);
        }
        if (skipAutoExpandProjectIdRef.current === projectId) {
          skipAutoExpandProjectIdRef.current = null;
        }
        return [...prev, key];
      });
      void selectProject(projectId);
    },
    [selectProject],
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

  const handleNewRecentThread = () => {
    void selectProject(null);
    window.dispatchEvent(new Event('aqbot:reset-agent-draft'));
  };

  // ── Conversations items — same shape as ChatSidebar getConversationItem ──
  const projectConversationItems: ConversationItemType[] = useMemo(() => {
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
            <span style={{ color: token.colorTextQuaternary, fontSize: 12 }}>
              {t('agentPage.emptyProjectThreads', '暂无对话')}
            </span>
          ),
          icon: null,
          style: { pointerEvents: 'none', minHeight: 28 },
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
          label: <SortableThreadLabel thread={th} title={th.title} />,
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

  const recentConversationItems: ConversationItemType[] = useMemo(() => {
    const items: ConversationItemType[] = [];
    for (const thread of filteredRecentThreads) {
      items.push({
        key: thread.id,
        icon: (
          <ThreadListIcon
            agentId={thread.agent_id}
            agentName={agentName(thread.agent_id)}
            agentIcon={agentIcon(thread.agent_id)}
            isStreaming={!!runningByThread[thread.id]}
            size={20}
          />
        ),
        label: <ThreadTitleText title={thread.title} />,
        'data-conv-id': thread.id,
      } as ConversationItemType);
    }
    return items;
  }, [
    agentName,
    agentIcon,
    runningByThread,
    filteredRecentThreads,
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
          revealLabel={revealLabel}
          deleteLabel={t('agentPage.deleteProject')}
          onSelect={() => handleSelectProject(projectId)}
          onNewThread={() => void handleNewThreadInProject(projectId)}
          onSettings={() => setSettingsProject(project)}
          onReveal={() => void handleRevealProject(project)}
          onDelete={() => handleDeleteProject(project)}
        />
      );
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      projectById,
      t,
      handleDeleteProject,
      handleNewThreadInProject,
      handleSelectProject,
      handleRevealProject,
      revealLabel,
    ],
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

  const buildThreadMenuItems = useCallback(
    (thread: AcpThread) => {
      const pinned = isThreadPinned(thread);
      const project = projectById.get(thread.project_id);
      return [
        {
          key: 'rename',
          icon: <Pencil size={14} />,
          label: t('chat.rename', '重命名'),
          onClick: () => handleRenameThread(thread),
        },
        {
          key: 'pin',
          icon: pinned ? <PinOff size={14} /> : <Pin size={14} />,
          label: pinned ? t('chat.unpin', '取消置顶') : t('chat.pin', '置顶'),
          onClick: () => {
            void toggleThreadPin(thread.id);
          },
        },
        {
          key: 'duplicate',
          icon: <Copy size={14} />,
          label: t('agentPage.copyThread', '复制对话'),
          onClick: () => handleDuplicateThread(thread),
        },
        {
          key: 'reveal',
          icon: <FolderOpen size={14} />,
          label: revealLabel,
          disabled: !project?.root_path,
          onClick: () => {
            if (project) void handleRevealProject(project);
          },
        },
        {
          key: 'delete',
          danger: true,
          icon: <Trash2 size={14} />,
          label: t('agentPage.deleteThread'),
          onClick: () => handleDeleteThread(thread),
        },
      ];
    },
    [
      projectById,
      t,
      handleRenameThread,
      toggleThreadPin,
      handleDuplicateThread,
      revealLabel,
      handleRevealProject,
      handleDeleteThread,
    ],
  );

  /** Hover ⋯ menu on each thread row (same actions as right-click). */
  const menuFactory = useCallback(
    (item: ConversationItemType) => {
      const id = String(item.key);
      if (id.startsWith('__')) return undefined;
      const thread =
        allThreads.find((th) => th.id === id)
        ?? threads.find((th) => th.id === id);
      if (!thread) return undefined;
      return { items: buildThreadMenuItems(thread) };
    },
    [allThreads, threads, buildThreadMenuItems],
  );

  /** List-level context menu (covers icon + padding, parity with chat). */
  const rightClickMenuConfig = useMemo(() => {
    if (!rightClickedThreadId) return { items: [] as ReturnType<typeof buildThreadMenuItems> };
    const thread =
      allThreads.find((th) => th.id === rightClickedThreadId)
      ?? threads.find((th) => th.id === rightClickedThreadId);
    if (!thread) return { items: [] as ReturnType<typeof buildThreadMenuItems> };
    return { items: buildThreadMenuItems(thread) };
  }, [rightClickedThreadId, allThreads, threads, buildThreadMenuItems]);

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
              disabled={agents.length === 0}
              onClick={handleNewRecentThread}
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
            <Dropdown
              menu={rightClickMenuConfig}
              trigger={['contextMenu']}
              onOpenChange={(open) => {
                if (!open) setRightClickedThreadId(null);
              }}
            >
              <div
                onContextMenu={(e) => {
                  // Only claim context menu on thread rows. Project group labels
                  // keep their own Dropdown — do not preventDefault on those.
                  const listItem = (e.target as HTMLElement).closest('[data-conv-id]') as HTMLElement | null;
                  if (!listItem) {
                    setRightClickedThreadId(null);
                    return;
                  }
                  const threadId = listItem.getAttribute('data-conv-id');
                  if (!threadId || threadId.startsWith('__')) {
                    setRightClickedThreadId(null);
                    return;
                  }
                  setRightClickedThreadId(threadId);
                }}
              >
            <DndContext
              sensors={dndSensors}
              collisionDetection={closestCenter}
              onDragStart={handleSidebarDragStart}
              onDragOver={handleSidebarDragOver}
              onDragEnd={handleSidebarDragEnd}
              onDragCancel={handleSidebarDragCancel}
            >
              <div
                style={{
                  alignItems: 'center',
                  color: token.colorTextSecondary,
                  display: 'flex',
                  fontSize: 13,
                  fontWeight: 600,
                  padding: '6px 8px 2px 12px',
                  width: '100%',
                }}
              >
                <button
                  type="button"
                  aria-expanded={expandedSections.projects}
                  onClick={() => setExpandedSections((current) => ({
                    ...current,
                    projects: !current.projects,
                  }))}
                  style={{
                    alignItems: 'center',
                    background: 'transparent',
                    border: 0,
                    color: 'inherit',
                    cursor: 'pointer',
                    display: 'flex',
                    flex: 1,
                    font: 'inherit',
                    fontWeight: 'inherit',
                    gap: 4,
                    minWidth: 0,
                    padding: '2px 0',
                    textAlign: 'left',
                  }}
                >
                  {expandedSections.projects
                    ? <ChevronDown size={14} />
                    : <ChevronRight size={14} />}
                  <span>{t('agentPage.projects', '项目')}</span>
                </button>
                <Tooltip title={t('agentPage.addProject')}>
                  <Button
                    type="text"
                    size="small"
                    icon={<Plus size={14} />}
                    aria-label={t('agentPage.addProject')}
                    onClick={() => void handleAddProject()}
                  />
                </Tooltip>
              </div>
              {expandedSections.projects ? (
                projectConversationItems.length > 0 ? (
                  <Conversations
                    items={projectConversationItems}
                    activeKey={activeThreadId ?? undefined}
                    onActiveChange={handleActiveChange}
                    groupable={groupableConfig}
                    menu={menuFactory}
                  />
                ) : userProjects.length === 0 && !query.trim() ? (
                  <div
                    style={{
                      color: token.colorTextQuaternary,
                      fontSize: 12,
                      padding: '6px 30px 10px',
                    }}
                  >
                    {t('agentPage.emptyProjects', '当前没有任何项目')}
                  </div>
                ) : null
              ) : null}

              <div
                style={{
                  alignItems: 'center',
                  color: token.colorTextSecondary,
                  display: 'flex',
                  fontSize: 13,
                  fontWeight: 600,
                  padding: '6px 8px 2px 12px',
                  width: '100%',
                }}
              >
                <button
                  type="button"
                  aria-expanded={expandedSections.recent}
                  onClick={() => setExpandedSections((current) => ({
                    ...current,
                    recent: !current.recent,
                  }))}
                  style={{
                    alignItems: 'center',
                    background: 'transparent',
                    border: 0,
                    color: 'inherit',
                    cursor: 'pointer',
                    display: 'flex',
                    flex: 1,
                    font: 'inherit',
                    fontWeight: 'inherit',
                    gap: 4,
                    minWidth: 0,
                    padding: '2px 0',
                    textAlign: 'left',
                  }}
                >
                  {expandedSections.recent
                    ? <ChevronDown size={14} />
                    : <ChevronRight size={14} />}
                  <span>{t('agentPage.recent', '最近')}</span>
                </button>
                <Tooltip title={t('agentPage.newThread')}>
                  <Button
                    type="text"
                    size="small"
                    icon={<Plus size={14} />}
                    aria-label={t('agentPage.newThread')}
                    onClick={handleNewRecentThread}
                  />
                </Tooltip>
              </div>
              {expandedSections.recent ? (
                recentConversationItems.length > 0 ? (
                  <Conversations
                    items={recentConversationItems}
                    activeKey={activeThreadId ?? undefined}
                    onActiveChange={handleActiveChange}
                    menu={menuFactory}
                  />
                ) : !query.trim() ? (
                  <div
                    style={{
                      color: token.colorTextQuaternary,
                      fontSize: 12,
                      padding: '6px 30px 10px',
                    }}
                  >
                    {t('agentPage.emptyRecentThreads', '暂无最近对话')}
                  </div>
                ) : null
              ) : null}
              <DragOverlay>
                {activeDragProjectId
                  ? (() => {
                      const project = projectById.get(activeDragProjectId);
                      if (!project) return null;
                      return (
                        <div
                          className="flex items-center gap-1.5"
                          style={{ opacity: 0.8, cursor: 'grabbing', fontSize: 13 }}
                        >
                          <GripVertical size={12} style={{ opacity: 0.4 }} />
                          <ProjectIcon projectId={project.id} size={20} />
                          <span>{project.name}</span>
                        </div>
                      );
                    })()
                  : activeDragThread
                    ? (
                        <div
                          className="flex items-center gap-1.5"
                          style={{
                            opacity: 0.85,
                            cursor: 'grabbing',
                            fontSize: 13,
                            padding: '4px 8px',
                            borderRadius: 6,
                            background: token.colorBgElevated,
                            boxShadow: token.boxShadowSecondary,
                          }}
                        >
                          <ThreadListIcon
                            agentId={activeDragThread.agent_id}
                            agentName={agentName(activeDragThread.agent_id)}
                            agentIcon={agentIcon(activeDragThread.agent_id)}
                            isStreaming={false}
                            size={18}
                          />
                          <span className="truncate" style={{ maxWidth: 180 }}>
                            {activeDragThread.title}
                          </span>
                        </div>
                      )
                    : null}
              </DragOverlay>
            </DndContext>
              </div>
            </Dropdown>
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
