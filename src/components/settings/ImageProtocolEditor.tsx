import { Alert, Checkbox, Form, Input, InputNumber, Select, Typography } from 'antd';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ImageAdapterConfig, ProviderType } from '@/types';
import { IMAGE_PARAM_PROFILES } from '@/types';

interface Props {
  value: ImageAdapterConfig | null | undefined;
  providerType: ProviderType;
  modelId: string;
  onChange: (value: ImageAdapterConfig | null) => void;
}

function isXaiImageModel(modelId: string): boolean {
  const normalized = modelId.trim().toLowerCase();
  return normalized.startsWith('grok-imagine') || normalized.startsWith('grok-image');
}

const PARAM_PROFILE_LABELS: Record<string, string> = {
  openai_gpt_image_2: 'gpt-image-2 (OpenAI)',
  openai_gpt_image_legacy: 'gpt-image-1.x (OpenAI)',
  openai_dalle_2: 'DALL·E 2',
  openai_dalle_3: 'DALL·E 3',
  xai_imagine: 'Grok Imagine (xAI)',
  gemini_3_1_flash: 'Gemini 3.1 Flash Image',
  gemini_3_1_flash_lite: 'Gemini 3.1 Flash Lite Image',
  gemini_3_pro: 'Gemini 3 Pro Image',
  gemini_2_5: 'Gemini 2.5 Flash Image',
  imagen_4: 'Imagen 4',
  imagen_4_ultra: 'Imagen 4 Ultra',
  imagen_4_fast: 'Imagen 4 Fast',
  glm_image: 'GLM Image',
  cogview: 'CogView',
  siliconflow_kolors: 'SiliconFlow Kolors',
  siliconflow_qwen: 'SiliconFlow Qwen Image',
  siliconflow_qwen_edit: 'SiliconFlow Qwen Image Edit',
};

export function ImageProtocolEditor({
  value,
  providerType,
  modelId,
  onChange,
}: Props) {
  const { t } = useTranslation();
  const config = value ?? {};
  const [mappingText, setMappingText] = useState(formatJson(config.mapping));
  const [extraBodyText, setExtraBodyText] = useState(formatJson(config.extra_body));
  const [jsonError, setJsonError] = useState<string | null>(null);

  useEffect(() => {
    setMappingText(formatJson(config.mapping));
    setExtraBodyText(formatJson(config.extra_body));
    setJsonError(null);
  }, [value]);

  const patch = (next: Partial<ImageAdapterConfig>) => {
    onChange({ ...config, ...next });
  };

  const commitJson = (field: 'mapping' | 'extra_body', text: string) => {
    let parsed: unknown;
    try {
      parsed = text.trim() ? JSON.parse(text) : {};
    } catch {
      setJsonError(t('imageProtocol.invalidJson'));
      return;
    }
    if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
      setJsonError(t('imageProtocol.jsonObjectRequired'));
      return;
    }
    setJsonError(null);
    patch({ [field]: parsed });
  };

  return (
    <div className="space-y-3">
      <div>
        <Typography.Title level={5} style={{ margin: 0 }}>
          {t('imageProtocol.title')}
        </Typography.Title>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {isXaiImageModel(modelId) || providerType === 'xai'
            ? t(
              'imageProtocol.grokAutoDetectDescription',
            )
            : t(
              'imageProtocol.autoDetectDescription',
            )}
        </Typography.Text>
      </div>

      <Form layout="vertical">
        <Form.Item label={t('imageProtocol.adapterProfile')}>
          <Select
            value={config.adapter_id ?? ''}
            options={[
              { value: '', label: t('imageProtocol.autoDetect') },
              { value: 'openai_images', label: 'OpenAI Images' },
              { value: 'xai_images', label: 'xAI Images' },
              { value: 'glm_images', label: 'GLM / CogView' },
              { value: 'siliconflow_images', label: 'SiliconFlow Images' },
              { value: 'gemini_images', label: 'Gemini / Nano Banana' },
              { value: 'generic_json', label: 'Generic JSON' },
            ]}
            onChange={(adapterId) => patch({ adapter_id: adapterId || null })}
          />
        </Form.Item>
        <Form.Item
          label={t('imageProtocol.paramProfile')}
          extra={t(
            'imageProtocol.paramProfileHint',
          )}
        >
          <Select
            value={config.param_profile ?? ''}
            options={[
              {
                value: '',
                label: t('imageProtocol.paramProfileAuto'),
              },
              ...IMAGE_PARAM_PROFILES.map((id) => ({
                value: id,
                label: t(`imageProtocol.paramProfile.${id}`, PARAM_PROFILE_LABELS[id] ?? id),
              })),
            ]}
            onChange={(profileId) => patch({ param_profile: profileId || null })}
          />
        </Form.Item>
        <Form.Item
          label={t(
            'imageProtocol.capabilityOverrides',
          )}
        >
          <Checkbox.Group
            value={config.operation_overrides ?? []}
            options={[
              { value: 'generate', label: t('imageProtocol.operation.generate') },
              { value: 'edit', label: t('imageProtocol.operation.edit') },
              { value: 'mask_edit', label: t('imageProtocol.operation.maskEdit') },
            ]}
            onChange={(operations) => patch({
              operation_overrides: operations.length > 0
                ? operations as ImageAdapterConfig['operation_overrides']
                : null,
            })}
          />
        </Form.Item>
        <Form.Item label={t('imageProtocol.generationEndpoint')}>
          <Input
            value={config.endpoint ?? ''}
            placeholder={t(
              'imageProtocol.generationEndpointPlaceholder',
            )}
            onChange={(event) => patch({ endpoint: event.target.value || null })}
          />
        </Form.Item>
        <Form.Item label={t('imageProtocol.editEndpoint')}>
          <Input
            value={config.edit_endpoint ?? ''}
            placeholder="/images/edits"
            onChange={(event) => patch({ edit_endpoint: event.target.value || null })}
          />
        </Form.Item>
        <Form.Item label={t('imageProtocol.pollEndpoint')}>
          <Input
            value={config.poll_endpoint ?? ''}
            placeholder="/tasks/{task_id}"
            onChange={(event) => patch({ poll_endpoint: event.target.value || null })}
          />
        </Form.Item>
        <Form.Item label={t('imageProtocol.cancelEndpoint')}>
          <Input
            value={config.cancel_endpoint ?? ''}
            placeholder="/tasks/{task_id}/cancel"
            onChange={(event) => patch({ cancel_endpoint: event.target.value || null })}
          />
        </Form.Item>
        <Form.Item label={t('imageProtocol.authMode')}>
          <Select
            value={config.auth_mode ?? 'bearer'}
            options={[
              { value: 'bearer', label: t('imageProtocol.auth.bearer') },
              {
                value: 'api_key_header',
                label: t('imageProtocol.auth.apiKeyHeader'),
              },
              { value: 'query', label: t('imageProtocol.auth.query') },
              { value: 'none', label: t('imageProtocol.auth.none') },
            ]}
            onChange={(authMode) => patch({ auth_mode: authMode })}
          />
        </Form.Item>
        {config.auth_mode === 'api_key_header' && (
          <Form.Item label={t('imageProtocol.authHeaderName')}>
            <Input
              value={config.auth_header ?? ''}
              placeholder="x-api-key"
              onChange={(event) => patch({ auth_header: event.target.value || null })}
            />
          </Form.Item>
        )}
        <div className="grid grid-cols-2 gap-3">
          <Form.Item label={t('imageProtocol.pollIntervalSeconds')}>
            <InputNumber
              min={1}
              max={30}
              value={config.poll_interval_secs ?? 3}
              style={{ width: '100%' }}
              onChange={(next) => patch({ poll_interval_secs: next ?? 3 })}
            />
          </Form.Item>
          <Form.Item label={t('imageProtocol.timeoutSeconds')}>
            <InputNumber
              min={60}
              max={86400}
              value={config.timeout_secs ?? 3600}
              style={{ width: '100%' }}
              onChange={(next) => patch({ timeout_secs: next ?? 3600 })}
            />
          </Form.Item>
        </div>
        <Form.Item label={t('imageProtocol.extraBody')}>
          <Input.TextArea
            value={extraBodyText}
            autoSize={{ minRows: 2, maxRows: 6 }}
            onChange={(event) => setExtraBodyText(event.target.value)}
            onBlur={() => commitJson('extra_body', extraBodyText)}
          />
        </Form.Item>
        {config.adapter_id === 'generic_json' && (
          <Form.Item
            label={t(
              'imageProtocol.fieldResponseMapping',
            )}
          >
            <Input.TextArea
              value={mappingText}
              autoSize={{ minRows: 5, maxRows: 12 }}
              onChange={(event) => setMappingText(event.target.value)}
              onBlur={() => commitJson('mapping', mappingText)}
              placeholder={'{"request_fields":{"prompt":"input.text"},"images_path":"/data"}'}
            />
          </Form.Item>
        )}
      </Form>
      {jsonError && (
        <Alert
          type="error"
          showIcon
          title={t('imageProtocol.invalidJson')}
          description={jsonError}
        />
      )}
    </div>
  );
}

function formatJson(value: unknown): string {
  if (!value || typeof value !== 'object') return '{}';
  return JSON.stringify(value, null, 2);
}
