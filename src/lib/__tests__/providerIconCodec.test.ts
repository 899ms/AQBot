import { describe, expect, it } from 'vitest';
import { encodeProviderIcon, parseProviderIcon } from '../providerIconCodec';

describe('parseProviderIcon', () => {
  it('returns null for empty values', () => {
    expect(parseProviderIcon(null)).toBeNull();
    expect(parseProviderIcon(undefined)).toBeNull();
    expect(parseProviderIcon('')).toBeNull();
  });

  it('treats bare keys as model_icon', () => {
    expect(parseProviderIcon('OpenAI')).toEqual({ type: 'model_icon', value: 'OpenAI' });
  });

  it('keeps model:/provider: prefixes as model_icon full value', () => {
    expect(parseProviderIcon('model:gpt-4')).toEqual({ type: 'model_icon', value: 'model:gpt-4' });
    expect(parseProviderIcon('provider:OpenAI')).toEqual({
      type: 'model_icon',
      value: 'provider:OpenAI',
    });
  });

  it('parses emoji / url / file prefixes', () => {
    expect(parseProviderIcon('emoji:😀')).toEqual({ type: 'emoji', value: '😀' });
    expect(parseProviderIcon('url:https://example.com/a.png')).toEqual({
      type: 'url',
      value: 'https://example.com/a.png',
    });
    expect(parseProviderIcon('file:images/abc_avatar-1.png')).toEqual({
      type: 'file',
      value: 'images/abc_avatar-1.png',
    });
  });

  it('preserves data URI after file: prefix', () => {
    const data = 'data:image/png;base64,iVBORw0KGgo=';
    expect(parseProviderIcon(`file:${data}`)).toEqual({ type: 'file', value: data });
  });
});

describe('encodeProviderIcon', () => {
  it('encodes model_icon without prefix rewrite', () => {
    expect(encodeProviderIcon('model_icon', 'provider:OpenAI')).toBe('provider:OpenAI');
    expect(encodeProviderIcon('model_icon', 'OpenAI')).toBe('OpenAI');
  });

  it('prefixes custom kinds', () => {
    expect(encodeProviderIcon('emoji', '😀')).toBe('emoji:😀');
    expect(encodeProviderIcon('url', 'https://x.test/a.png')).toBe('url:https://x.test/a.png');
    expect(encodeProviderIcon('file', 'images/a.png')).toBe('file:images/a.png');
  });

  it('clears on null/empty', () => {
    expect(encodeProviderIcon(null, null)).toBe('');
    expect(encodeProviderIcon('file', null)).toBe('');
    expect(encodeProviderIcon('file', '')).toBe('');
    expect(encodeProviderIcon(null, 'x')).toBe('');
  });
});
