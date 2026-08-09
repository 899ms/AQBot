import { describe, expect, it } from 'vitest';
import type { AppSettings } from '@/types';
import {
  DEFAULT_SHORTCUT_BINDINGS,
  SHORTCUT_ACTIONS,
  SHORTCUT_SETTING_ACTIONS,
  getShortcutBinding,
  isShortcutEnabled,
  matchesShortcutEvent,
} from '../shortcuts';

describe('shortcuts', () => {
  it('exposes send message as a configurable input shortcut without global handling', () => {
    expect(DEFAULT_SHORTCUT_BINDINGS.sendMessage).toBe('Enter');
    expect(SHORTCUT_SETTING_ACTIONS).toContain('sendMessage');
    expect(SHORTCUT_ACTIONS).not.toContain('sendMessage');
  });

  it('matches the configured send message shortcut', () => {
    const defaultBinding = DEFAULT_SHORTCUT_BINDINGS.sendMessage;

    expect(matchesShortcutEvent(new KeyboardEvent('keydown', { key: 'Enter' }), defaultBinding)).toBe(true);
    expect(matchesShortcutEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true }), defaultBinding)).toBe(false);

    const settings = { shortcut_send_message: 'CmdOrCtrl+Enter' } as AppSettings;
    const customBinding = getShortcutBinding(settings, 'sendMessage');

    expect(matchesShortcutEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true }), customBinding)).toBe(true);
    expect(matchesShortcutEvent(new KeyboardEvent('keydown', { key: 'Enter' }), customBinding)).toBe(false);
  });

  it('uses defaults when the binding field is missing', () => {
    const settings = {} as AppSettings;
    expect(getShortcutBinding(settings, 'newConversation')).toBe(DEFAULT_SHORTCUT_BINDINGS.newConversation);
    expect(isShortcutEnabled(settings, 'newConversation')).toBe(true);
  });

  it('treats an explicit empty string as disabled', () => {
    const settings = { shortcut_new_conversation: '' } as AppSettings;
    expect(getShortcutBinding(settings, 'newConversation')).toBe('');
    expect(isShortcutEnabled(settings, 'newConversation')).toBe(false);
  });
});
