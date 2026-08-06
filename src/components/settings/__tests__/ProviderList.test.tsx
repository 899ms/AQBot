import { App } from 'antd';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReactNode } from 'react';
import type { ProviderConfig, ProviderImportCandidate } from '@/types';
import { ProviderList } from '../ProviderList';

const mocks = vi.hoisted(() => ({
  createProvider: vi.fn(),
  fetchProviders: vi.fn(),
  scanCcSwitchProviderImports: vi.fn(),
  importCcSwitchProviderConfigs: vi.fn(),
  toggleProvider: vi.fn(),
  deleteProvider: vi.fn(),
  reorderProviders: vi.fn(),
  setSelectedProviderId: vi.fn(),
}));

function makeProvider(overrides: Partial<ProviderConfig>): ProviderConfig {
  return {
    id: 'provider-1',
    name: 'OpenAI',
    provider_type: 'openai',
    api_host: 'https://api.openai.com',
    api_path: null,
    aws_region: null,
    enabled: true,
    models: [],
    keys: [],
    proxy_config: null,
    custom_headers: null,
    icon: null,
    builtin_id: null,
    sort_order: 0,
    created_at: 0,
    updated_at: 0,
    ...overrides,
  };
}

function makeCandidate(overrides: Partial<ProviderImportCandidate>): ProviderImportCandidate {
  return {
    id: 'candidate-1',
    source_app: 'cc-switch',
    name: 'Claude Relay',
    provider_type: 'anthropic',
    api_host: 'https://api.anthropic.com',
    api_path: '/v1/messages',
    key_prefix: 'sk-ant...',
    models: ['claude-sonnet'],
    status: 'ready',
    reason: null,
    ...overrides,
  };
}

let providers: ProviderConfig[] = [];
let selectedProviderId: string | null = null;

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string | Record<string, unknown>) =>
      key === 'settings.builtinProviderBadge'
        ? 'Built-in Label'
        : (typeof fallback === 'string' ? fallback : key),
  }),
}));

vi.mock('antd', async (importOriginal) => {
  const actual = await importOriginal<typeof import('antd')>();
  return {
    ...actual,
    Select: ({
      options = [],
      value,
      onChange,
      id,
      disabled,
    }: {
      options?: Array<{ label: string; value: string }>;
      value?: string;
      onChange?: (value: string) => void;
      id?: string;
      disabled?: boolean;
    }) => (
      <select
        id={id}
        disabled={disabled}
        value={value ?? ''}
        onChange={(event) => onChange?.(event.target.value)}
      >
        <option value="" />
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    ),
  };
});

vi.mock('@/lib/providerIcons', () => ({
  SmartProviderIcon: () => <span data-testid="provider-icon" />,
}));

vi.mock('@dnd-kit/core', () => ({
  DndContext: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  closestCenter: vi.fn(),
  PointerSensor: vi.fn(),
  useSensor: vi.fn(() => ({})),
  useSensors: vi.fn(() => []),
}));

vi.mock('@dnd-kit/sortable', () => ({
  SortableContext: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  verticalListSortingStrategy: {},
  useSortable: vi.fn(() => ({
    attributes: {},
    listeners: {},
    setNodeRef: vi.fn(),
    transform: null,
    transition: undefined,
    isDragging: false,
  })),
}));

vi.mock('@dnd-kit/utilities', () => ({
  CSS: {
    Transform: {
      toString: vi.fn(() => ''),
    },
  },
}));

vi.mock('@/stores', () => ({
  useProviderStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({
      providers,
      createProvider: mocks.createProvider,
      fetchProviders: mocks.fetchProviders,
      scanCcSwitchProviderImports: mocks.scanCcSwitchProviderImports,
      importCcSwitchProviderConfigs: mocks.importCcSwitchProviderConfigs,
      toggleProvider: mocks.toggleProvider,
      deleteProvider: mocks.deleteProvider,
      reorderProviders: mocks.reorderProviders,
    }),
  useUIStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({
      selectedProviderId,
      setSelectedProviderId: mocks.setSelectedProviderId,
    }),
}));

describe('ProviderList', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    selectedProviderId = 'builtin-openai';
    providers = [
      makeProvider({ id: 'builtin-openai', name: 'OpenAI', builtin_id: 'openai' }),
      makeProvider({ id: 'custom-openai', name: 'Custom OpenAI', builtin_id: null }),
    ];
    mocks.scanCcSwitchProviderImports.mockResolvedValue([]);
    mocks.importCcSwitchProviderConfigs.mockResolvedValue({
      created_count: 0,
      added_key_count: 0,
      reused_count: 0,
      skipped_count: 0,
      provider_ids: [],
    });
  });

  it('shows the built-in badge only next to built-in providers', () => {
    render(
      <App>
        <ProviderList />
      </App>,
    );

    expect(screen.getByLabelText('Built-in Label')).toBeInTheDocument();
    expect(screen.getByText('OpenAI')).toBeInTheDocument();
    expect(screen.getByText('Custom OpenAI')).toBeInTheDocument();
    expect(screen.getAllByTestId('provider-icon')).toHaveLength(2);
  });

  it('keeps the selected built-in provider when its virtual id is materialized', () => {
    const firstProvider = makeProvider({
      id: 'real-openai',
      name: 'OpenAI',
      builtin_id: 'openai',
    });
    selectedProviderId = 'builtin_shuaiapi';
    providers = [
      firstProvider,
      makeProvider({
        id: 'builtin_shuaiapi',
        name: 'SHUAI API',
        builtin_id: 'shuaiapi',
      }),
    ];
    const { rerender } = render(
      <App>
        <ProviderList />
      </App>,
    );

    providers = [
      firstProvider,
      makeProvider({
        id: 'real-shuaiapi',
        name: 'SHUAI API',
        builtin_id: 'shuaiapi',
      }),
    ];
    rerender(
      <App>
        <ProviderList />
      </App>,
    );

    expect(mocks.setSelectedProviderId).toHaveBeenCalledWith('real-shuaiapi');
    expect(mocks.setSelectedProviderId).not.toHaveBeenCalledWith('real-openai');
  });

  it('selects the first provider when the selected provider is genuinely missing', () => {
    selectedProviderId = 'deleted-custom-provider';
    providers = [
      makeProvider({ id: 'real-openai', name: 'OpenAI', builtin_id: 'openai' }),
      makeProvider({ id: 'custom-openai', name: 'Custom OpenAI', builtin_id: null }),
    ];

    render(
      <App>
        <ProviderList />
      </App>,
    );

    expect(mocks.setSelectedProviderId).toHaveBeenCalledWith('real-openai');
  });

  it('enters batch mode and can bulk-disable selected providers', async () => {
    const user = userEvent.setup();
    mocks.toggleProvider.mockResolvedValue(undefined);
    providers = [
      makeProvider({ id: 'p1', name: 'Provider One', enabled: true, builtin_id: null }),
      makeProvider({ id: 'p2', name: 'Provider Two', enabled: true, builtin_id: null }),
    ];
    selectedProviderId = 'p1';

    render(
      <App>
        <ProviderList />
      </App>,
    );

    await user.click(screen.getByRole('button', { name: 'settings.providerBatchMode' }));
    expect(screen.getByLabelText('common.selectAll')).toBeInTheDocument();

    const checkboxes = screen.getAllByRole('checkbox');
    // first is select-all; remaining are provider rows
    await user.click(checkboxes[1]);
    await user.click(checkboxes[2]);
    await user.click(screen.getByRole('button', { name: 'settings.batchDisable' }));

    await waitFor(() => {
      expect(mocks.toggleProvider).toHaveBeenCalledWith('p1', false);
      expect(mocks.toggleProvider).toHaveBeenCalledWith('p2', false);
    });
  });

  it('batch delete skips built-in providers and only deletes custom ones', async () => {
    const user = userEvent.setup();
    mocks.deleteProvider.mockResolvedValue(undefined);
    providers = [
      makeProvider({ id: 'builtin-openai', name: 'OpenAI', enabled: true, builtin_id: 'openai' }),
      makeProvider({ id: 'custom-openai', name: 'Custom OpenAI', enabled: true, builtin_id: null }),
    ];
    selectedProviderId = 'custom-openai';

    render(
      <App>
        <ProviderList />
      </App>,
    );

    await user.click(screen.getByRole('button', { name: 'settings.providerBatchMode' }));
    await user.click(screen.getByLabelText('common.selectAll'));

    // Popconfirm needs confirm click
    await user.click(screen.getByRole('button', { name: 'settings.batchDeleteBtn' }));
    const confirm = await screen.findByRole('button', { name: 'common.confirm' });
    await user.click(confirm);

    await waitFor(() => {
      expect(mocks.deleteProvider).toHaveBeenCalledTimes(1);
      expect(mocks.deleteProvider).toHaveBeenCalledWith('custom-openai');
    });
  });

  it('requires Region and hides API Host when adding AWS Bedrock', async () => {
    const user = userEvent.setup();
    mocks.createProvider.mockResolvedValue(
      makeProvider({
        id: 'bedrock-1',
        name: 'AWS Bedrock',
        provider_type: 'bedrock',
        api_host: '',
        aws_region: 'us-west-2',
      }),
    );
    render(
      <App>
        <ProviderList />
      </App>,
    );

    await user.click(screen.getByRole('button', { name: 'settings.addProvider' }));
    const dialog = await screen.findByRole('dialog');
    const providerTypeSelect = within(dialog).getByRole('combobox');
    await user.selectOptions(providerTypeSelect, 'bedrock');

    expect(within(dialog).getByText('settings.awsRegion')).toBeInTheDocument();
    expect(within(dialog).queryByText('settings.apiHost')).not.toBeInTheDocument();

    await user.type(within(dialog).getByRole('textbox'), 'AWS Bedrock');
    await user.type(within(dialog).getAllByRole('combobox')[1], 'us-west-2');
    await user.click(within(dialog).getByRole('button', { name: 'common.confirm' }));

    await waitFor(() => {
      expect(mocks.createProvider).toHaveBeenCalledWith({
        name: 'AWS Bedrock',
        provider_type: 'bedrock',
        api_host: '',
        aws_region: 'us-west-2',
        enabled: true,
      });
    });
  });

  it('shows an import icon after the add provider button and scans from the dropdown', async () => {
    const user = userEvent.setup();
    render(
      <App>
        <ProviderList />
      </App>,
    );

    const addButton = screen.getByRole('button', { name: 'settings.addProvider' });
    const importButton = screen.getByRole('button', { name: 'settings.importProviders' });
    const toolbarButtons = screen.getAllByRole('button');
    expect(toolbarButtons.indexOf(importButton)).toBeGreaterThan(toolbarButtons.indexOf(addButton));

    await user.click(importButton);
    await user.click(await screen.findByText('settings.importFromCcSwitch'));

    expect(mocks.scanCcSwitchProviderImports).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('settings.ccSwitchImportTitle')).toBeInTheDocument();
  });

  it('defaults selectable import candidates and submits selected ids', async () => {
    const user = userEvent.setup();
    mocks.scanCcSwitchProviderImports.mockResolvedValue([
      makeCandidate({ id: 'ready-1', name: 'Ready Provider', status: 'ready' }),
      makeCandidate({ id: 'add-key-1', name: 'Add Key Provider', status: 'add_key' }),
      makeCandidate({ id: 'existing-1', name: 'Existing Provider', status: 'already_exists' }),
      makeCandidate({
        id: 'unsupported-1',
        name: 'OAuth Provider',
        status: 'unsupported',
        reason: 'OAuth providers cannot be imported',
      }),
    ]);
    mocks.importCcSwitchProviderConfigs.mockResolvedValue({
      created_count: 1,
      added_key_count: 1,
      reused_count: 0,
      skipped_count: 0,
      provider_ids: ['provider-1'],
    });

    render(
      <App>
        <ProviderList />
      </App>,
    );

    await user.click(screen.getByRole('button', { name: 'settings.importProviders' }));
    await user.click(await screen.findByText('settings.importFromCcSwitch'));

    expect(await screen.findByText('Ready Provider')).toBeInTheDocument();
    expect(screen.getByText('Add Key Provider')).toBeInTheDocument();
    expect(screen.getByText('OAuth providers cannot be imported')).toBeInTheDocument();

    const unsupportedRow = screen.getByText('OAuth Provider').closest('tr');
    expect(unsupportedRow).not.toBeNull();
    expect(within(unsupportedRow as HTMLTableRowElement).getByRole('checkbox')).toBeDisabled();

    await user.click(screen.getByRole('button', { name: 'common.confirm' }));

    await waitFor(() => {
      expect(mocks.importCcSwitchProviderConfigs).toHaveBeenCalledWith(['ready-1', 'add-key-1']);
    });
    expect(mocks.setSelectedProviderId).toHaveBeenCalledWith('provider-1');
  });

  it('toggles batch selection when clicking the provider row, not only the checkbox', async () => {
    const user = userEvent.setup();
    mocks.toggleProvider.mockResolvedValue(undefined);
    providers = [
      makeProvider({ id: 'p1', name: 'Provider One', enabled: true, builtin_id: null }),
      makeProvider({ id: 'p2', name: 'Provider Two', enabled: true, builtin_id: null }),
    ];
    selectedProviderId = 'p1';

    render(
      <App>
        <ProviderList />
      </App>,
    );

    await user.click(screen.getByRole('button', { name: 'settings.providerBatchMode' }));
    await user.click(screen.getByText('Provider One'));
    await user.click(screen.getByRole('button', { name: 'settings.batchDisable' }));

    await waitFor(() => {
      expect(mocks.toggleProvider).toHaveBeenCalledWith('p1', false);
    });
    expect(mocks.toggleProvider).not.toHaveBeenCalledWith('p2', false);
  });

  it('disables a provider from the context menu', async () => {
    const user = userEvent.setup();
    mocks.toggleProvider.mockResolvedValue(undefined);
    providers = [
      makeProvider({ id: 'p1', name: 'Provider One', enabled: true, builtin_id: null }),
    ];
    selectedProviderId = 'p1';

    render(
      <App>
        <ProviderList />
      </App>,
    );

    fireEvent.contextMenu(screen.getByText('Provider One'));
    await user.click(await screen.findByText('停用服务商'));

    await waitFor(() => {
      expect(mocks.toggleProvider).toHaveBeenCalledWith('p1', false);
    });
  });

  it('deletes a custom provider from the context menu after modal confirm', async () => {
    const user = userEvent.setup();
    mocks.deleteProvider.mockResolvedValue(undefined);
    providers = [
      makeProvider({ id: 'custom-1', name: 'Custom Provider', enabled: true, builtin_id: null }),
      makeProvider({ id: 'other-1', name: 'Other Provider', enabled: true, builtin_id: null }),
    ];
    selectedProviderId = 'custom-1';

    render(
      <App>
        <ProviderList />
      </App>,
    );

    fireEvent.contextMenu(screen.getByText('Custom Provider'));
    await user.click(await screen.findByText('settings.deleteProvider'));

    const confirm = await screen.findByRole('button', { name: 'common.confirm' });
    await user.click(confirm);

    await waitFor(() => {
      expect(mocks.deleteProvider).toHaveBeenCalledWith('custom-1');
    });
    expect(mocks.setSelectedProviderId).toHaveBeenCalledWith('other-1');
  });

  it('hides delete from the context menu for built-in providers', async () => {
    providers = [
      makeProvider({ id: 'builtin-openai', name: 'OpenAI', enabled: true, builtin_id: 'openai' }),
    ];
    selectedProviderId = 'builtin-openai';

    render(
      <App>
        <ProviderList />
      </App>,
    );

    fireEvent.contextMenu(screen.getByText('OpenAI'));
    expect(await screen.findByText('停用服务商')).toBeInTheDocument();
    expect(screen.queryByText('settings.deleteProvider')).not.toBeInTheDocument();
  });
});
