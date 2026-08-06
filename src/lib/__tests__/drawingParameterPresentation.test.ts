import { describe, expect, it } from 'vitest';
import {
  getDrawingParameterAutoCompleteOption,
  getDrawingParameterLabel,
  getDrawingParameterOption,
  resolveDrawingParameterProtocolValue,
} from '../drawingParameterPresentation';

const labels: Record<string, string> = {
  'drawing.aspectRatio': '宽高比',
  'drawing.size': '尺寸',
  'drawing.batchCount': '批量张数',
  'drawing.referenceImageFormat': '参考图格式',
  'drawing.personGeneration': '人物生成',
  'drawing.seed': '随机种子',
  'drawing.option.auto': '自动',
  'drawing.option.quality.standard': '标准',
  'drawing.option.quality.hd': '高清',
  'drawing.option.quality.low': '低',
  'drawing.option.background.opaque': '不透明',
  'drawing.option.background.transparent': '透明',
  'drawing.option.outputFormat.png': 'PNG',
  'drawing.option.referenceImageMode.base64': 'Base64',
  'drawing.option.referenceImageFormat.object': '对象数组',
  'drawing.option.personGeneration.allowAdult': '允许成人',
  'drawing.option.personGeneration.dontAllow': '不允许',
};
const translate = (key: string, fallback: string) => labels[key] ?? fallback;

describe('drawing parameter presentation', () => {
  it('separates localized labels from protocol values', () => {
    expect(getDrawingParameterLabel('aspect_ratio', translate)).toBe('宽高比');
    expect(getDrawingParameterLabel('custom_parameter', translate)).toBe('custom_parameter');
    expect(getDrawingParameterOption('size', 'auto', translate)).toEqual({
      label: '自动',
      value: 'auto',
    });
    expect(getDrawingParameterOption('size', '1024x1024', translate)).toEqual({
      label: '1024x1024',
      value: '1024x1024',
    });
    expect(getDrawingParameterOption('output_format', 'png', translate)).toEqual({
      label: 'PNG',
      value: 'png',
    });
    expect(getDrawingParameterOption('custom_parameter', 'custom_value', translate)).toEqual({
      label: 'custom_value',
      value: 'custom_value',
    });
  });

  it('localizes known parameter aliases and semantic values', () => {
    expect(getDrawingParameterLabel('image_size', translate)).toBe('尺寸');
    expect(getDrawingParameterLabel('batch_size', translate)).toBe('批量张数');
    expect(getDrawingParameterLabel('reference_image_format', translate)).toBe('参考图格式');
    expect(getDrawingParameterOption('quality', 'standard', translate).label).toBe('标准');
    expect(getDrawingParameterOption('quality', 'hd', translate).label).toBe('高清');
    expect(getDrawingParameterOption('quality', 'low', translate).label).toBe('低');
    expect(getDrawingParameterOption('background', 'opaque', translate).label).toBe('不透明');
    expect(getDrawingParameterOption('background', 'transparent', translate).label).toBe('透明');
    expect(getDrawingParameterOption('reference_image_mode', 'base64', translate)).toEqual({
      label: 'Base64',
      value: 'base64',
    });
    expect(getDrawingParameterOption('reference_image_format', 'object', translate)).toEqual({
      label: '对象数组',
      value: 'object',
    });
    expect(getDrawingParameterOption('aspect_ratio', '16:9', translate).label).toBe('16:9');
    expect(getDrawingParameterLabel('person_generation', translate)).toBe('人物生成');
    expect(getDrawingParameterLabel('seed', translate)).toBe('随机种子');
    expect(getDrawingParameterOption('person_generation', 'allow_adult', translate).label)
      .toBe('允许成人');
    expect(getDrawingParameterOption('person_generation', 'dont_allow', translate).label)
      .toBe('不允许');
  });

  it('maps auto-complete display labels back to protocol values', () => {
    const sizeOptions = ['auto', '1024x1024', '1536x1024'];
    expect(getDrawingParameterAutoCompleteOption('size', 'auto', translate)).toEqual({
      value: '自动',
      label: '自动',
    });
    expect(resolveDrawingParameterProtocolValue('size', '自动', sizeOptions, translate))
      .toBe('auto');
    expect(resolveDrawingParameterProtocolValue('size', 'auto', sizeOptions, translate))
      .toBe('auto');
    expect(resolveDrawingParameterProtocolValue('size', '1024x1024', sizeOptions, translate))
      .toBe('1024x1024');
    expect(resolveDrawingParameterProtocolValue('size', '2048x1024', sizeOptions, translate))
      .toBe('2048x1024');
    expect(resolveDrawingParameterProtocolValue('quality', '标准', ['auto', 'standard', 'hd'], translate))
      .toBe('standard');
  });
});
