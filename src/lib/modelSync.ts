import type { Model, ModelSyncStatus } from '@/types';

export const MODEL_SYNC_STATUS_CONFIG: Record<ModelSyncStatus, { color: string; labelKey: string }> = {
  synced: { color: 'blue', labelKey: 'settings.modelAlreadyAdded' },
  'local-only': { color: 'gold', labelKey: 'settings.remoteMissing' },
  'remote-only': { color: 'green', labelKey: 'settings.remoteAvailable' },
  unsupported: { color: 'red', labelKey: 'settings.modelUnsupported' },
};

export function deriveModelGroupName(modelId: string): string {
  const parts = modelId
    .trim()
    .split('-')
    .filter((part) => part.length > 0);

  if (parts.length >= 2) return parts.slice(0, 2).join('-');
  if (parts.length === 1) return parts[0];
  return modelId.trim();
}

export function getModelGroupName(model: Pick<Model, 'model_id' | 'group_name'>): string {
  const explicitGroup = model.group_name?.trim();
  return explicitGroup || deriveModelGroupName(model.model_id);
}

export function formatTokenCount(tokens: number): string {
  if (tokens >= 1000000) {
    const m = tokens / 1000000;
    return m % 1 === 0 ? `${m}M` : `${m.toFixed(1)}M`;
  }
  if (tokens >= 1000) {
    const k = tokens / 1000;
    return k % 1 === 0 ? `${k}K` : `${k.toFixed(1)}K`;
  }
  return `${tokens}`;
}
