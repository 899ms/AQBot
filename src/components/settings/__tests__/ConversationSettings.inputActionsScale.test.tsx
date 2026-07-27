import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useSettingsStore } from '@/stores';
import { ConversationSettings } from '../ConversationSettings';

vi.mock('@/lib/invoke', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
  isTauri: () => false,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key === 'settings.chatInputActionsScale'
      ? '底部操作区缩放'
      : key,
  }),
}));

describe('ConversationSettings input actions scale', () => {
  beforeEach(() => {
    useSettingsStore.setState((state) => ({
      settings: {
        ...state.settings,
        chat_input_actions_scale: 100,
      },
      _loaded: true,
      error: null,
    }));
  });

  it('allows clearing and replacing the controlled value', async () => {
    const user = userEvent.setup();
    render(<ConversationSettings />);

    const input = screen.getByRole('spinbutton', { name: '底部操作区缩放' });
    await user.clear(input);

    expect((input as HTMLInputElement).value).toBe('');

    await user.type(input, '50');

    expect((input as HTMLInputElement).value).toBe('50');
    expect(useSettingsStore.getState().settings.chat_input_actions_scale).toBe(50);

    const inputNumber = input.closest('.ant-input-number');
    expect(inputNumber).not.toBeNull();
    await user.click(within(inputNumber as HTMLElement).getByRole('button', {
      name: 'Increase Value',
    }));

    expect((input as HTMLInputElement).value).toBe('60');
    expect(useSettingsStore.getState().settings.chat_input_actions_scale).toBe(60);
  });
});
