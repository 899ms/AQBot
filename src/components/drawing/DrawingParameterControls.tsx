import {
  AutoComplete,
  Form,
  Input,
  InputNumber,
  Select,
  Slider,
  Switch,
  Typography,
} from 'antd';
import type { ReactNode } from 'react';
import type {
  DrawingSettings,
  DrawingTarget,
  ImageParameterDescriptor,
} from '@/types';
import {
  getDrawingParameterAutoCompleteOption,
  getDrawingParameterLabel,
  getDrawingParameterOption,
  getDrawingParameterValueLabel,
  resolveDrawingParameterProtocolValue,
} from '@/lib/drawingParameterPresentation';
import { DrawingReferenceUploader } from './DrawingReferenceUploader';
import type {
  DrawingParamField,
  DrawingParamOption,
  DrawingParamRenderContext,
} from './params/types';

interface FieldProps {
  field: DrawingParamField;
  settings: DrawingSettings;
  context: DrawingParamRenderContext;
  target?: DrawingTarget;
  providerOptions: Array<{ label: ReactNode; value: string }>;
  secondaryTextColor: string;
  translate: (key: string, fallback: string) => string;
  /** Extra control rendered immediately after the field label (e.g. model compat notice). */
  labelAccessory?: ReactNode;
  onChange: (field: DrawingParamField, value: unknown) => void;
  onProviderChange: (providerId: string) => void;
}

function formLabel(text: string, accessory?: ReactNode): ReactNode {
  if (!accessory) return text;
  // Keep the chip tight against the label text (not right-aligned).
  return (
    <span className="inline-flex items-center gap-1.5">
      <span>{text}</span>
      {accessory}
    </span>
  );
}

export function DrawingParameterField({
  field,
  settings,
  context,
  target,
  providerOptions,
  secondaryTextColor,
  translate,
  labelAccessory,
  onChange,
  onProviderChange,
}: FieldProps) {
  const label = translate(field.labelKey, field.fallbackLabel);
  const descriptor = getDescriptorParameter(field, target);
  switch (field.type) {
    case 'modelSelect':
      return (
        <Form.Item label={formLabel(label, labelAccessory)}>
          <Select
            value={settings.modelId}
            options={toSelectOptions(context.modelOptions, translate)}
            placeholder={translate('drawing.selectModel', 'Select drawing model')}
            onChange={(value) => onChange(field, value)}
          />
        </Form.Item>
      );
    case 'providerSelect':
      return (
        <Form.Item label={label}>
          <Select
            value={settings.providerId || undefined}
            placeholder={translate('drawing.selectProvider', 'Select provider')}
            options={providerOptions}
            optionLabelProp="label"
            onChange={onProviderChange}
          />
        </Form.Item>
      );
    case 'select':
      if (descriptor?.kind === 'string') {
        const parameterKey = descriptor.key;
        const protocolValue = field.key
          ? String(settings[field.key] ?? descriptor.default ?? '')
          : '';
        return (
          <Form.Item label={label}>
            <AutoComplete
              value={getDrawingParameterValueLabel(parameterKey, protocolValue, translate)}
              options={descriptor.options.map((value) => (
                getDrawingParameterAutoCompleteOption(parameterKey, value, translate)
              ))}
              onChange={(displayValue) => onChange(
                field,
                resolveDrawingParameterProtocolValue(
                  parameterKey,
                  displayValue,
                  descriptor.options,
                  translate,
                ),
              )}
            />
          </Form.Item>
        );
      }
      return (
        <Form.Item label={label}>
          <Select
            value={field.key ? settings[field.key] : undefined}
          options={
            getDescriptorOptions(field, target, translate)
            ?? toSelectOptions(resolveOptions(field, context), translate)
          }
            onChange={(value) => onChange(field, value)}
          />
        </Form.Item>
      );
    case 'number':
      return (
        <Form.Item label={label}>
          <InputNumber
            min={descriptor?.min ?? field.min}
            max={descriptor?.max ?? field.max}
            value={field.key ? Number(settings[field.key]) : field.defaultValue}
            style={{ width: '100%' }}
            onChange={(value) => onChange(
              field,
              value ?? descriptor?.default ?? field.defaultValue ?? field.min ?? 0,
            )}
          />
        </Form.Item>
      );
    case 'text':
      return (
        <Form.Item label={label}>
          <Input
            value={field.key ? String(settings[field.key] ?? '') : ''}
            placeholder={field.placeholder}
            onChange={(event) => onChange(field, event.target.value)}
          />
          {field.fallbackHint && (
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {translate(field.hintKey ?? field.id, field.fallbackHint ?? '')}
            </Typography.Text>
          )}
        </Form.Item>
      );
    case 'compression':
      return (
        <Form.Item label={label}>
          <div className="flex items-center gap-3">
            <Switch
              checked={settings.outputCompression !== undefined}
              onChange={(checked) => onChange(field, checked ? field.defaultValue ?? 90 : undefined)}
            />
            <Slider
              min={descriptor?.min ?? field.min ?? 0}
              max={descriptor?.max ?? field.max ?? 100}
              disabled={settings.outputCompression === undefined}
              value={settings.outputCompression ?? field.defaultValue ?? 90}
              onChange={(value) => onChange(field, value)}
              style={{ flex: 1 }}
            />
          </div>
        </Form.Item>
      );
    case 'referenceUploader':
      return (
        <div>
          <Typography.Text style={{ fontSize: 12, color: secondaryTextColor }}>
            {label}
          </Typography.Text>
          <div className="mb-4 mt-2">
            <DrawingReferenceUploader />
          </div>
        </div>
      );
    default:
      return null;
  }
}

interface DynamicProps {
  parameters: ImageParameterDescriptor[];
  values: Record<string, unknown>;
  translate: (key: string, fallback: string) => string;
  onChange: (key: string, value: unknown) => void;
}

export function DrawingDynamicParameters({
  parameters,
  values,
  translate,
  onChange,
}: DynamicProps) {
  return parameters.map((parameter) => (
    <Form.Item
      key={parameter.key}
      label={getDrawingParameterLabel(parameter.key, translate)}
    >
      {parameter.kind === 'select' ? (
        <Select
          value={values[parameter.key] ?? parameter.default}
          options={parameter.options.map((value) => (
            getDrawingParameterOption(parameter.key, value, translate)
          ))}
          onChange={(value) => onChange(parameter.key, value)}
        />
      ) : parameter.kind === 'number' ? (
        <InputNumber
          min={parameter.min ?? undefined}
          max={parameter.max ?? undefined}
          value={Number(values[parameter.key] ?? parameter.default)}
          style={{ width: '100%' }}
          onChange={(value) => onChange(parameter.key, value ?? parameter.default)}
        />
      ) : parameter.kind === 'boolean' ? (
        <Switch
          checked={Boolean(values[parameter.key] ?? parameter.default)}
          onChange={(value) => onChange(parameter.key, value)}
        />
      ) : (
        <AutoComplete
          value={getDrawingParameterValueLabel(
            parameter.key,
            values[parameter.key] ?? parameter.default ?? '',
            translate,
          )}
          options={parameter.options.map((value) => (
            getDrawingParameterAutoCompleteOption(parameter.key, value, translate)
          ))}
          onChange={(displayValue) => onChange(
            parameter.key,
            resolveDrawingParameterProtocolValue(
              parameter.key,
              displayValue,
              parameter.options,
              translate,
            ),
          )}
        />
      )}
    </Form.Item>
  ));
}

function getDescriptorOptions(
  field: DrawingParamField,
  target: DrawingTarget | undefined,
  translate: (key: string, fallback: string) => string,
): Array<{ label: string; value: unknown }> | undefined {
  const parameter = getDescriptorParameter(field, target);
  if (!parameter || parameter.options.length === 0) return undefined;
  return parameter.options.map((value) => (
    getDrawingParameterOption(parameter.key, value, translate)
  ));
}

function getDescriptorParameter(
  field: DrawingParamField,
  target: DrawingTarget | undefined,
): ImageParameterDescriptor | undefined {
  if (!target) return undefined;
  const keys: Partial<Record<string, string>> = {
    size: 'size',
    quality: 'quality',
    outputFormat: 'output_format',
    background: 'background',
    batchCount: 'n',
    compression: 'output_compression',
  };
  const key = keys[field.id];
  if (!key) return undefined;
  return target.descriptor.parameters.find((item) => item.key === key);
}

function resolveOptions(
  field: DrawingParamField,
  context: DrawingParamRenderContext,
): readonly DrawingParamOption[] {
  if (!field.options) return [];
  return typeof field.options === 'function' ? field.options(context) : field.options;
}

function toSelectOptions(
  options: readonly DrawingParamOption[],
  translate: (key: string, fallback: string) => string,
) {
  return options.map((option) => ({
    label: option.labelKey
      ? translate(option.labelKey, option.fallbackLabel)
      : option.fallbackLabel,
    value: option.value,
  }));
}
