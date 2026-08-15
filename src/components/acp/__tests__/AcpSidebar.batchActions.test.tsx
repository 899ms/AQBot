import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AcpSidebar } from '../AcpSidebar';
import type { AcpProject, AcpThread, ConfiguredAgent } from '@/types/acp';

const mocks = vi.hoisted(() => ({
  confirm: vi.fn(),
  deleteThread: vi.fn(),
  batchDeleteThreads: vi.fn(),
  selectThread: vi.fn(),
  selectProject: vi.fn(),
  loadProjects: vi.fn(),
  loadAllThreads: vi.fn(),
  setProjectsOrder: vi.fn(),
  reorderProjects: vi.fn(),
  setThreadsOrder: vi.fn(),
  reorderThreads: vi.fn(),
  createProject: vi.fn(),
  deleteProject: vi.fn(),
  renameThread: vi.fn(),
  toggleThreadPin: vi.fn(),
  duplicateThread: vi.fn(),
  ensureRecentDraft: vi.fn(),
  setActivePage: vi.fn(),
  setSettingsSection: vi.fn(),
}));

const agent: ConfiguredAgent = {
  id: 'grok-build',
  name: 'Grok Build',
  enabled: true,
  source: 'registry',
  command: 'grok',
  args: ['acp'],
  sort: 0,
};

const recentProject: AcpProject = {
  id: 'recent-1',
  name: 'Recent',
  root_path: '/tmp/recent-1',
  kind: 'recent',
  sort_order: 0,
  created_at: '2026-08-08T00:00:00Z',
  updated_at: '2026-08-08T00:00:00Z',
};

function makeThread(overrides: Partial<AcpThread> = {}): AcpThread {
  return {
    id: 'thread-1',
    project_id: 'recent-1',
    agent_id: 'grok-build',
    title: '快捷删除测试',
    runtime_status: 'idle',
    mode_id: null,
    is_pinned: 0,
    sort_order: 0,
    created_at: '2026-08-08T00:00:00Z',
    updated_at: '2026-08-08T00:00:00Z',
    ...overrides,
  };
}

const acpState: any = {
  projects: [recentProject],
  allThreads: [makeThread()],
  threads: [],
  activeProjectId: 'recent-1',
  activeThreadId: 'thread-1',
  runningByThread: {},
  configReady: true,
  projectsReady: true,
  threadsReady: true,
  composerSubmitting: false,
  creatingThread: false,
  enabledAgents: () => [agent],
  loadProjects: mocks.loadProjects,
  loadAllThreads: mocks.loadAllThreads,
  setProjectsOrder: mocks.setProjectsOrder,
  reorderProjects: mocks.reorderProjects,
  setThreadsOrder: mocks.setThreadsOrder,
  reorderThreads: mocks.reorderThreads,
  createProject: mocks.createProject,
  deleteProject: mocks.deleteProject,
  selectProject: mocks.selectProject,
  selectThread: mocks.selectThread,
  deleteThread: mocks.deleteThread,
  batchDeleteThreads: mocks.batchDeleteThreads,
  renameThread: mocks.renameThread,
  toggleThreadPin: mocks.toggleThreadPin,
  duplicateThread: mocks.duplicateThread,
  ensureRecentDraft: mocks.ensureRecentDraft,
};

const uiState = {
  setActivePage: mocks.setActivePage,
  setSettingsSection: mocks.setSettingsSection,
};

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, string | number>) => ({
      'chat.searchPlaceholder': '搜索对话...',
      'chat.rename': '重命名',
      'chat.pin': '置顶',
      'chat.unpin': '取消置顶',
      'chat.delete': '删除',
      'chat.deleteConfirm': '确定删除此对话？',
      'chat.directDeleteHint': `按住 ${options?.shortcut ?? 'Ctrl'} 可直接删除`,
      'chat.multiSelect': '多选',
      'chat.selectAll': '全选',
      'chat.selected': '已选',
      'chat.batchDeleteContent': `确定删除 ${options?.count ?? 0} 个对话？`,
      'common.cancel': '取消',
      'common.confirm': '确定',
      'agentPage.addProject': '添加项目',
      'agentPage.newThread': '新建对话',
      'agentPage.projects': '项目',
      'agentPage.recent': '最近',
      'agentPage.emptyProjects': '当前没有任何项目',
      'agentPage.emptyRecentThreads': '暂无最近对话',
      'agentPage.emptyProjectThreads': '暂无对话',
      'agentPage.deleteThread': '删除对话',
      'agentPage.deleteProject': '删除项目',
      'agentPage.projectSettings': '项目设置',
      'agentPage.copyThread': '复制对话',
      'agentPage.copyThreadSuffix': ' (副本)',
      'agentPage.copyThreadSuccess': '已复制对话',
      'agentPage.showInFolder': '在文件夹中显示',
      'agentPage.showInFinder': '在 Finder 中显示',
      'agentPage.loading': '加载 Agent 配置中…',
      'agentPage.noAgents': '请先在 设置 → ACP Agent 中启用至少一个 Agent',
      'agentPage.openSettings': '打开 ACP 设置',
      'settings.acpAgents.title': 'ACP Agent',
    }[key] ?? key),
  }),
}));

vi.mock('antd', () => ({
  App: {
    useApp: () => ({
      message: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
      modal: { confirm: mocks.confirm },
    }),
  },
  Button: ({ children, icon, onClick, 'aria-label': ariaLabel, title, disabled }: any) => (
    <button type="button" aria-label={ariaLabel ?? title} disabled={disabled} onClick={onClick}>
      {icon}
      {children}
    </button>
  ),
  Input: (props: any) => <input {...props} />,
  Tooltip: ({ children, title, ...triggerProps }: any) => (
    <span {...triggerProps} title={typeof title === 'string' ? title : undefined}>{children}</span>
  ),
  Checkbox: ({ checked, onChange, onClick, indeterminate }: any) => (
    <input
      type="checkbox"
      checked={checked}
      data-indeterminate={indeterminate ? 'true' : undefined}
      onChange={onChange}
      onClick={onClick}
      readOnly
    />
  ),
  Dropdown: ({ children, menu }: any) => (
    <div>
      {children}
      {menu?.items?.map((item: any) => (
        <button
          key={item.key}
          type="button"
          aria-label={typeof item.label === 'string' ? item.label : undefined}
          disabled={item.disabled}
          onClick={(event) => {
            item.onClick?.({ domEvent: event });
            menu.onClick?.({ key: item.key, domEvent: event });
          }}
        >
          {item.icon}
          {item.label}
        </button>
      ))}
    </div>
  ),
  Empty: ({ description }: any) => <div>{description}</div>,
  Spin: ({ tip }: any) => <div>{tip}</div>,
  theme: {
    useToken: () => ({
      token: {
        colorPrimary: '#1677ff',
        colorPrimaryBg: '#e6f4ff',
        colorBgContainer: '#fff',
        colorFillContent: '#f5f5f5',
        colorTextSecondary: '#666',
        colorTextQuaternary: '#aaa',
        colorBgElevated: '#fff',
        boxShadowSecondary: 'none',
      },
    }),
  },
}));

vi.mock('@ant-design/x/es/conversations', () => ({
  default: ({ items, menu, activeKey, onActiveChange }: any) => (
    <ul>
      {items.map((item: any) => {
        const menuConfig = typeof menu === 'function' ? menu(item) : menu;
        const originNode = <button type="button" aria-label="更多" />;
        const trigger = typeof menuConfig?.trigger === 'function'
          ? menuConfig.trigger(item, { originNode })
          : menuConfig?.trigger ?? originNode;

        return (
          <li
            key={item.key}
            data-conv-id={item['data-conv-id']}
            className={activeKey === item.key ? 'ant-conversations-item-active' : undefined}
            onClick={() => onActiveChange?.(item.key, item)}
          >
            {item.icon}
            {item.label}
            {menuConfig && trigger}
            {menuConfig?.items?.map((menuItem: any) => (
              <button
                key={menuItem.key}
                type="button"
                aria-label={typeof menuItem.label === 'string' ? `菜单${menuItem.label}` : undefined}
                disabled={menuItem.disabled}
                onClick={(event) => {
                  event.stopPropagation();
                  menuItem.onClick?.({ domEvent: event });
                  menuConfig.onClick?.({ key: menuItem.key, domEvent: event });
                }}
              >
                {menuItem.icon}
                {menuItem.label}
              </button>
            ))}
          </li>
        );
      })}
    </ul>
  ),
}));

vi.mock('@dnd-kit/core', () => ({
  DndContext: ({ children }: any) => <>{children}</>,
  DragOverlay: ({ children }: any) => <>{children}</>,
  closestCenter: vi.fn(),
  PointerSensor: vi.fn(),
  useSensor: vi.fn(() => ({})),
  useSensors: vi.fn(() => []),
  useDraggable: () => ({
    attributes: {},
    listeners: {},
    setNodeRef: vi.fn(),
    isDragging: false,
  }),
  useDroppable: () => ({
    setNodeRef: vi.fn(),
  }),
}));

vi.mock('@/stores/acpStore', () => ({
  useAcpStore: Object.assign(
    (selector: (state: typeof acpState) => unknown) => selector(acpState),
    { getState: () => acpState },
  ),
}));

vi.mock('@/stores', () => ({
  useUIStore: (selector: (state: typeof uiState) => unknown) => selector(uiState),
}));

vi.mock('@/hooks/useResolvedAvatarSrc', () => ({
  useResolvedAvatarSrc: () => null,
}));

vi.mock('@/lib/acpAgentIcon', () => ({
  AcpAgentIcon: () => <span data-testid="agent-icon" />,
}));

vi.mock('@/lib/acpProjectIcon', () => ({
  getAcpProjectIcon: () => null,
}));

vi.mock('@/components/shared/DynamicLobeIcon', () => ({
  DynamicLobeIcon: () => null,
}));

vi.mock('../AcpProjectSettingsModal', () => ({
  AcpProjectSettingsModal: () => null,
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
}));

function armThreadMenu(title = '快捷删除测试') {
  const row = screen.getByText(title).closest('li');
  expect(row).not.toBeNull();
  fireEvent.pointerOver(row!);
}

describe('AcpSidebar batch actions and direct delete', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    acpState.projects = [recentProject];
    acpState.allThreads = [makeThread()];
    acpState.threads = [];
    acpState.activeProjectId = 'recent-1';
    acpState.activeThreadId = 'thread-1';
    acpState.runningByThread = {};
    mocks.loadProjects.mockResolvedValue(undefined);
    mocks.loadAllThreads.mockResolvedValue(undefined);
    mocks.deleteThread.mockResolvedValue(undefined);
    mocks.batchDeleteThreads.mockResolvedValue(undefined);
  });

  it('keeps the confirmation dialog for a normal menu delete click', async () => {
    render(<AcpSidebar />);

    armThreadMenu();
    fireEvent.click(screen.getByRole('button', { name: '更多' }));
    fireEvent.click(await screen.findByRole('button', { name: '菜单删除对话' }));

    expect(mocks.confirm).toHaveBeenCalledTimes(1);
    expect(mocks.deleteThread).not.toHaveBeenCalled();
  });

  it('turns the more trigger into direct delete while Cmd is held', async () => {
    render(<AcpSidebar />);
    armThreadMenu();

    fireEvent.keyDown(window, { key: 'Meta', metaKey: true });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: '删除' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '删除' }), { metaKey: true });

    expect(mocks.confirm).not.toHaveBeenCalled();
    expect(mocks.deleteThread).toHaveBeenCalledWith('thread-1');
  });

  it('turns the more trigger into direct delete while Ctrl is held', async () => {
    render(<AcpSidebar />);
    armThreadMenu();

    fireEvent.keyDown(window, { key: 'Control', ctrlKey: true });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: '删除' })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: '删除' }), { ctrlKey: true });

    expect(mocks.confirm).not.toHaveBeenCalled();
    expect(mocks.deleteThread).toHaveBeenCalledWith('thread-1');
  });

  it('skips confirmation when the menu delete is clicked with Cmd held', async () => {
    render(<AcpSidebar />);
    armThreadMenu();

    fireEvent.click(screen.getByRole('button', { name: '菜单删除对话' }), { metaKey: true });

    expect(mocks.confirm).not.toHaveBeenCalled();
    expect(mocks.deleteThread).toHaveBeenCalledWith('thread-1');
  });

  it('lets users multi-select visible threads and batch delete them', async () => {
    acpState.allThreads = [
      makeThread({ id: 'thread-1', title: '第一条' }),
      makeThread({ id: 'thread-2', title: '第二条', sort_order: 1 }),
    ];

    render(<AcpSidebar />);

    fireEvent.click(screen.getByRole('button', { name: '多选' }));
    fireEvent.click(screen.getByText('第一条'));
    fireEvent.click(screen.getByText('第二条'));

    expect(screen.getByText('2 已选')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '删除' }));

    expect(mocks.confirm).toHaveBeenCalledTimes(1);
    expect(mocks.confirm.mock.calls[0][0]).toEqual(expect.objectContaining({
      title: '确定删除此对话？',
      content: '确定删除 2 个对话？',
    }));

    await act(async () => {
      await mocks.confirm.mock.calls[0][0].onOk();
    });

    expect(mocks.batchDeleteThreads).toHaveBeenCalledWith(['thread-1', 'thread-2']);
  });

  it('selects every visible thread from the toolbar checkbox', async () => {
    acpState.allThreads = [
      makeThread({ id: 'thread-1', title: '第一条' }),
      makeThread({ id: 'thread-2', title: '第二条', sort_order: 1 }),
    ];

    render(<AcpSidebar />);
    fireEvent.click(screen.getByRole('button', { name: '多选' }));
    fireEvent.click(screen.getByTitle('全选').querySelector('input')!);

    expect(screen.getByText('2 已选')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '删除' }));
    await act(async () => {
      await mocks.confirm.mock.calls[0][0].onOk();
    });
    expect(mocks.batchDeleteThreads).toHaveBeenCalledWith(['thread-1', 'thread-2']);
  });
});
