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

function isRerankModel(model: { model_id: string; model_type?: string }) {
  return model.model_type === 'Rerank' || /rerank|colbert/i.test(model.model_id);
}

function useRerankModelOptions() {
  const providers = useProviderStore((s) => s.providers);
  const ensureProvidersLoaded = useProviderStore((s) => s.ensureProvidersLoaded);

  useEffect(() => {
    void ensureProvidersLoaded();
  }, [ensureProvidersLoaded]);

  return useMemo(() => {
    return providers
      .filter((p) => p.enabled)
      .map((p) => {
        const rerankModels = p.models.filter((m) => m.enabled && isRerankModel(m));
        if (rerankModels.length === 0) return null;
        return {
          label: (
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
              <ModelIcon model={p.name} size={16} type="avatar" />
              {p.name}
            </span>
          ),
          title: p.name,
          options: rerankModels.map((m) => ({
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

export function RerankModelSelect({
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
  const rerankOptions = useRerankModelOptions();
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
      options={rerankOptions}
      style={style}
    />
  );
}
