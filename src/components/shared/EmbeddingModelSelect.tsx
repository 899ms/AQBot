import { useMemo, useEffect, type CSSProperties } from 'react';
import { Select } from 'antd';
import { ModelIcon } from '@lobehub/icons';
import { useProviderStore } from '@/stores';
import {
  MODEL_SELECT_CLASS,
  useProviderNameMap,
  useModelSelectLabelRender,
  useModelSelectOptionRender,
} from './ModelSelect';

function isEmbeddingModel(model: { model_id: string; model_type?: string }) {
  return model.model_type === 'Embedding' || /embed/i.test(model.model_id);
}

/** Hook: returns grouped Select options filtered to embedding-capable models */
function useEmbeddingModelOptions() {
  const providers = useProviderStore((s) => s.providers);
  const ensureProvidersLoaded = useProviderStore((s) => s.ensureProvidersLoaded);

  useEffect(() => {
    void ensureProvidersLoaded();
  }, [ensureProvidersLoaded]);

  return useMemo(() => {
    return providers
      .filter((p) => p.enabled)
      .map((p) => {
        const embeddingModels = p.models.filter(
          (m) => m.enabled && isEmbeddingModel(m),
        );
        if (embeddingModels.length === 0) return null;
        return {
          label: (
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
              <ModelIcon model={p.name} size={16} type="avatar" />
              {p.name}
            </span>
          ),
          title: p.name,
          options: embeddingModels.map((m) => ({
            label: m.name,
            value: `${p.id}::${m.model_id}`,
            modelId: m.model_id,
            providerName: p.name,
          })),
        };
      })
      .filter((opt): opt is NonNullable<typeof opt> => opt !== null);
  }, [providers]);
}

/**
 * Model selector filtered to embedding-capable models.
 */
export function EmbeddingModelSelect({
  value,
  onChange,
  placeholder,
  allowClear = true,
  style,
  className,
}: {
  value?: string;
  onChange: (value: string | undefined) => void;
  placeholder?: string;
  allowClear?: boolean;
  style?: CSSProperties;
  className?: string;
}) {
  const embeddingOptions = useEmbeddingModelOptions();
  const providerNameMap = useProviderNameMap();
  const optionRender = useModelSelectOptionRender();
  const labelRender = useModelSelectLabelRender(providerNameMap);

  return (
    <Select
      className={[MODEL_SELECT_CLASS, className].filter(Boolean).join(' ')}
      value={value}
      onChange={onChange}
      placeholder={placeholder}
      allowClear={allowClear}
      showSearch
      optionFilterProp="label"
      optionRender={optionRender}
      labelRender={labelRender}
      options={embeddingOptions}
      style={style}
    />
  );
}
