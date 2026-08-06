import { describe, expect, it } from 'vitest';
import { getRoleErrorMessage, validateRoleDraft } from '../roleErrorMessage';

const t = ((key: string, opts?: { detail?: string }) => {
  const map: Record<string, string> = {
    'roles.validation.nameRequired': '请输入角色名称',
    'roles.validation.systemPromptRequired': '请输入系统提示词',
    'roles.validation.failed': `校验失败：${opts?.detail ?? ''}`,
    'roles.saveFailed': '保存角色失败',
    'roles.notFound': '角色不存在',
  };
  return map[key] ?? key;
}) as import('i18next').TFunction;

describe('getRoleErrorMessage', () => {
  it('localizes backend name validation errors', () => {
    expect(getRoleErrorMessage('Validation error: name cannot be empty', t)).toBe('请输入角色名称');
  });

  it('localizes backend system_prompt validation errors', () => {
    expect(getRoleErrorMessage('Validation error: system_prompt cannot be empty', t)).toBe(
      '请输入系统提示词',
    );
  });

  it('wraps unknown validation errors', () => {
    expect(getRoleErrorMessage('Validation error: tags invalid', t)).toBe('校验失败：tags invalid');
  });

  it('localizes not-found errors', () => {
    expect(getRoleErrorMessage('Not found: Role abc', t)).toBe('角色不存在');
  });

  it('passes through unknown messages', () => {
    expect(getRoleErrorMessage('network down', t)).toBe('network down');
  });
});

describe('validateRoleDraft', () => {
  it('requires name and system prompt', () => {
    expect(validateRoleDraft({ name: '  ', systemPrompt: '' }, t)).toEqual({
      name: '请输入角色名称',
      systemPrompt: '请输入系统提示词',
    });
    expect(validateRoleDraft({ name: '助手', systemPrompt: '你是助手' }, t)).toEqual({});
  });
});
