import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Model, ModelMetadataChange, ModelSyncStatus } from '@/types';
import { ModelSyncPickerModal, type ModelSyncEntry } from '../ModelSyncPickerModal';

vi.setConfig({ testTimeout: 15000 });

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string | Record<string, unknown>) =>
      typeof fallback === 'string' ? fallback : key,
  }),
}));

vi.mock('@lobehub/icons', () => ({
  ModelIcon: () => <div>model-icon</div>,
  ProviderIcon: () => <div>provider-icon</div>,
  modelMappings: [],
  providerMappings: [],
}));

vi.mock('@/lib/providerIcons', () => ({
  SmartModelIcon: () => <div>smart-model-icon</div>,
  SmartProviderIcon: () => <div>smart-provider-icon</div>,
}));

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count, getItemKey }: { count: number; getItemKey?: (index: number) => string }) => ({
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        index,
        key: getItemKey ? getItemKey(index) : index,
        start: index * 48,
      })),
    getTotalSize: () => count * 48,
    measure: () => {},
    measureElement: () => {},
  }),
}));

function makeModel(modelId: string, group = 'group-a'): Model {
  return {
    provider_id: 'provider-1',
    model_id: modelId,
    name: modelId,
    group_name: group,
    model_type: 'Chat',
    capabilities: ['TextChat'],
    context_window: null,
    enabled: true,
    param_overrides: null,
  };
}

function makeEntry(
  model: Model,
  status: ModelSyncStatus,
  changes: ModelMetadataChange[] = [],
): ModelSyncEntry {
  return {
    proposed_model: model,
    model,
    status,
    catalog_mode: null,
    inference_source: 'catalog',
    changes,
    unsupported_reason: null,
  };
}

describe('ModelSyncPickerModal', () => {
  it('preselects added models, reports additions in the footer, and applies the final list', async () => {
    const local = makeModel('local-model');
    const remote = makeModel('remote-model');
    const onApply = vi.fn();
    render(
      <ModelSyncPickerModal
        open
        entries={[makeEntry(local, 'synced'), makeEntry(remote, 'remote-only')]}
        catalog={null}
        localModels={[local]}
        onCancel={() => {}}
        onApply={onApply}
      />,
    );

    expect(screen.getByRole('checkbox', { name: 'local-model' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'remote-model' })).not.toBeChecked();
    expect(screen.getByText('settings.syncImpactNone')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('checkbox', { name: 'remote-model' }));
    expect(screen.getByText('settings.syncImpactAdd')).toBeInTheDocument();
    expect(screen.queryByText('settings.syncImpactNone')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'settings.applyModelSync' }));
    await waitFor(() => {
      expect(onApply).toHaveBeenCalledWith([local, remote]);
    });
  });

  it('warns about removals and only applies after confirmation', async () => {
    const keep = makeModel('keep-model');
    const drop = makeModel('drop-model');
    const onApply = vi.fn();
    render(
      <ModelSyncPickerModal
        open
        entries={[makeEntry(keep, 'synced'), makeEntry(drop, 'synced')]}
        catalog={null}
        localModels={[keep, drop]}
        onCancel={() => {}}
        onApply={onApply}
      />,
    );

    await userEvent.click(screen.getByRole('checkbox', { name: 'drop-model' }));
    expect(screen.getByText('settings.syncImpactRemove')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'settings.applyModelSync' }));
    expect(onApply).not.toHaveBeenCalled();
    expect(await screen.findByText('settings.syncRemoveConfirmTitle')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'common.confirm' }));
    await waitFor(() => {
      expect(onApply).toHaveBeenCalledWith([keep]);
    });
  });

  it('filters the list by sync status', async () => {
    const local = makeModel('local-model');
    const remote = makeModel('remote-model');
    render(
      <ModelSyncPickerModal
        open
        entries={[makeEntry(local, 'synced'), makeEntry(remote, 'remote-only')]}
        catalog={null}
        localModels={[local]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    await userEvent.click(screen.getByText(/settings\.syncFilterNew/));
    expect(screen.getByRole('checkbox', { name: 'remote-model' })).toBeInTheDocument();
    expect(screen.queryByRole('checkbox', { name: 'local-model' })).not.toBeInTheDocument();

    await userEvent.click(screen.getByText(/settings\.syncFilterAll/));
    expect(screen.getByRole('checkbox', { name: 'local-model' })).toBeInTheDocument();
  });

  it('toggles selection by clicking anywhere on the row', async () => {
    const remote = makeModel('remote-model');
    render(
      <ModelSyncPickerModal
        open
        entries={[makeEntry(remote, 'remote-only')]}
        catalog={null}
        localModels={[]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    const checkbox = screen.getByRole('checkbox', { name: 'remote-model' });
    expect(checkbox).not.toBeChecked();
    await userEvent.click(screen.getByText('remote-model'));
    expect(checkbox).toBeChecked();
    await userEvent.click(screen.getByText('remote-model'));
    expect(checkbox).not.toBeChecked();
  });

  it('selects a range with shift-click', async () => {
    const models = ['model-a', 'model-b', 'model-c'].map((id) => makeModel(id));
    render(
      <ModelSyncPickerModal
        open
        entries={models.map((model) => makeEntry(model, 'remote-only'))}
        catalog={null}
        localModels={[]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    await userEvent.click(screen.getByRole('checkbox', { name: 'model-a' }));
    const lastRow = screen.getByText('model-c').closest('.model-sync-row');
    expect(lastRow).not.toBeNull();
    fireEvent.click(lastRow as HTMLElement, { shiftKey: true });

    expect(screen.getByRole('checkbox', { name: 'model-a' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'model-b' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'model-c' })).toBeChecked();
  });

  it('selects a range when shift-clicking the checkbox itself', async () => {
    const models = ['model-a', 'model-b', 'model-c'].map((id) => makeModel(id));
    render(
      <ModelSyncPickerModal
        open
        entries={models.map((model) => makeEntry(model, 'remote-only'))}
        catalog={null}
        localModels={[]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    await userEvent.click(screen.getByRole('checkbox', { name: 'model-a' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'model-c' }), { shiftKey: true });

    expect(screen.getByRole('checkbox', { name: 'model-a' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'model-b' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'model-c' })).toBeChecked();
  });

  it('selects all new models from the quick-select menu', async () => {
    const local = makeModel('local-model');
    const remoteA = makeModel('remote-a');
    const remoteB = makeModel('remote-b');
    render(
      <ModelSyncPickerModal
        open
        entries={[
          makeEntry(local, 'synced'),
          makeEntry(remoteA, 'remote-only'),
          makeEntry(remoteB, 'remote-only'),
        ]}
        catalog={null}
        localModels={[local]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    await userEvent.click(screen.getByRole('button', { name: 'settings.quickSelect' }));
    await userEvent.click(await screen.findByText('settings.syncSelectAllNew'));

    expect(screen.getByRole('checkbox', { name: 'remote-a' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'remote-b' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'local-model' })).toBeChecked();
  });

  it('shows the selected count per group in the group header', async () => {
    const modelA = makeModel('model-a');
    const modelB = makeModel('model-b');
    render(
      <ModelSyncPickerModal
        open
        entries={[makeEntry(modelA, 'synced'), makeEntry(modelB, 'remote-only')]}
        catalog={null}
        localModels={[modelA]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    const groupHeader = screen.getByText('group-a').closest('.model-sync-group');
    expect(groupHeader).not.toBeNull();
    expect(within(groupHeader as HTMLElement).getByText('1/2')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('checkbox', { name: 'model-b' }));
    expect(within(groupHeader as HTMLElement).getByText('2/2')).toBeInTheDocument();
  });

  it('shows a single capability summary tag instead of individual capability tags', () => {
    const multiCap = {
      ...makeModel('multi-cap'),
      capabilities: ['TextChat', 'Vision', 'Reasoning'] as Model['capabilities'],
    };
    const noCap = {
      ...makeModel('no-cap'),
      capabilities: [] as Model['capabilities'],
    };
    render(
      <ModelSyncPickerModal
        open
        entries={[makeEntry(multiCap, 'synced'), makeEntry(noCap, 'remote-only')]}
        catalog={null}
        localModels={[multiCap]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    // Mock t returns the key when no string fallback is provided
    expect(screen.getByLabelText('settings.capabilityCount')).toBeInTheDocument();
    expect(screen.getAllByLabelText('settings.capabilityCount')).toHaveLength(1);
    // Individual capability enum labels must not appear as list tags
    expect(screen.queryByText('TextChat')).not.toBeInTheDocument();
    expect(screen.queryByText('Vision')).not.toBeInTheDocument();
    expect(screen.queryByText('Reasoning')).not.toBeInTheDocument();
  });

  it('does not render metadata change summaries under model rows', () => {
    const model = makeModel('meta-change');
    render(
      <ModelSyncPickerModal
        open
        entries={[
          makeEntry(model, 'synced', [
            {
              field: 'model_type',
              previous: 'Chat',
              proposed: 'Embedding',
              source: 'catalog',
            },
            {
              field: 'capabilities',
              previous: ['TextChat'],
              proposed: ['TextChat', 'Vision'],
              source: 'catalog',
            },
          ]),
        ]}
        catalog={null}
        localModels={[model]}
        onCancel={() => {}}
        onApply={() => {}}
      />,
    );

    expect(screen.queryByText('settings.metadataSyncField.model_type')).not.toBeInTheDocument();
    expect(screen.queryByText('settings.metadataSyncField.capabilities')).not.toBeInTheDocument();
    expect(screen.queryByText('model_type')).not.toBeInTheDocument();
    expect(screen.queryByText(/Chat.*Embedding|Embedding/)).not.toBeInTheDocument();
  });

  it('keeps unsupported local models when applying without them selected', async () => {
    const unsupportedLocal = makeModel('unsupported-model');
    const supported = makeModel('supported-model');
    const onApply = vi.fn();
    render(
      <ModelSyncPickerModal
        open
        entries={[
          {
            ...makeEntry(unsupportedLocal, 'unsupported'),
            unsupported_reason: 'not supported',
          },
          makeEntry(supported, 'synced'),
        ]}
        catalog={null}
        localModels={[unsupportedLocal, supported]}
        onCancel={() => {}}
        onApply={onApply}
      />,
    );

    expect(screen.getByRole('checkbox', { name: 'unsupported-model' })).toBeDisabled();
    expect(screen.getByText('settings.syncImpactNone')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'settings.applyModelSync' }));
    await waitFor(() => {
      expect(onApply).toHaveBeenCalledWith([supported, unsupportedLocal]);
    });
  });
});
