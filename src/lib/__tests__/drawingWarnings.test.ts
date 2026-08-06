import { describe, expect, it } from 'vitest';
import {
  getDrawingWarningDescription,
  getDrawingWarningTitle,
  isDrawingCompatibilityNotice,
  splitDrawingWarnings,
} from '../drawingWarnings';
import type { ImageModelWarning } from '@/types';

const labels: Record<string, string> = {
  'drawing.warning.unknown_image_profile':
    '{{modelId}} 尚未验证图片参数配置，当前仅允许保守的文生图请求。',
  'drawing.warning.using_fallback_profile':
    '{{modelId}} 尚未验证图片参数配置，已使用适配器默认参数预设。',
  'drawing.warning.compatibilityTitle': '兼容提示',
  'drawing.warning.legacy_model':
    '{{modelId}} 是旧版图片模型；新项目请使用 {{replacement}}。',
  'drawing.warning.retired_model':
    '{{modelId}} 是已退役的预览模型。兼容代理仍可继续请求。',
  'drawing.warning.deprecated_model':
    '该图片模型已弃用并计划下线。兼容端点仍可使用。',
  'drawing.warning.deadline': '截止日期：{{deadline}}',
  'drawing.warning.replacement': '建议模型：{{modelId}}',
  'drawing.warning.separator': '；',
};

function translate(
  key: string,
  options: Record<string, unknown>,
): string {
  const template = labels[key] ?? String(options.defaultValue ?? key);
  return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) =>
    String(options[name] ?? ''),
  );
}

describe('drawingWarnings', () => {
  it('localizes known warning codes with model id interpolation', () => {
    const warning: ImageModelWarning = {
      code: 'unknown_image_profile',
      message: 'vendor-image-model has no verified image parameter profile; only conservative text-to-image requests are enabled.',
      deadline: null,
      replacement_model_id: null,
    };

    expect(getDrawingWarningTitle(warning, 'vendor-image-model', translate)).toBe(
      'vendor-image-model 尚未验证图片参数配置，当前仅允许保守的文生图请求。',
    );
    expect(getDrawingWarningDescription(warning, translate)).toBeUndefined();
  });

  it('localizes fallback parameter profile notices', () => {
    const warning: ImageModelWarning = {
      code: 'using_fallback_profile',
      message:
        'gemini-3.1-flash-image has no verified image parameter profile; using fallback parameter preset `openai_gpt_image_2`.',
      deadline: null,
      replacement_model_id: null,
    };

    expect(getDrawingWarningTitle(warning, 'gemini-3.1-flash-image', translate)).toBe(
      'gemini-3.1-flash-image 尚未验证图片参数配置，已使用适配器默认参数预设。',
    );
  });

  it('localizes deadline and replacement metadata', () => {
    const warning: ImageModelWarning = {
      code: 'retired_model',
      message: 'This preview model is retired.',
      deadline: '2026-01-15',
      replacement_model_id: 'gemini-3.1-flash-image',
    };

    expect(getDrawingWarningTitle(warning, 'gemini-2.5-flash-image-preview', translate)).toBe(
      'gemini-2.5-flash-image-preview 是已退役的预览模型。兼容代理仍可继续请求。',
    );
    expect(getDrawingWarningDescription(warning, translate)).toBe(
      '截止日期：2026-01-15；建议模型：gemini-3.1-flash-image',
    );
  });

  it('falls back to backend message for unknown codes', () => {
    const warning: ImageModelWarning = {
      code: 'custom_backend_code',
      message: 'Backend-only English warning.',
      deadline: null,
      replacement_model_id: null,
    };

    expect(getDrawingWarningTitle(warning, 'any-model', translate)).toBe(
      'Backend-only English warning.',
    );
  });

  it('classifies soft profile notices for compact model-label UI', () => {
    const fallback: ImageModelWarning = {
      code: 'using_fallback_profile',
      message: 'fallback',
      deadline: null,
      replacement_model_id: null,
    };
    const retired: ImageModelWarning = {
      code: 'retired_model',
      message: 'retired',
      deadline: null,
      replacement_model_id: null,
    };
    expect(isDrawingCompatibilityNotice(fallback)).toBe(true);
    expect(isDrawingCompatibilityNotice(retired)).toBe(false);
    expect(splitDrawingWarnings([fallback, retired])).toEqual({
      compatibilityNotices: [fallback],
      blockWarnings: [retired],
    });
  });
});
