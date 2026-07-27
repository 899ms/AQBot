import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
}));

describe('chat input actions scale settings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it('normalizes a fetched scale to the supported step', async () => {
    invokeMock.mockResolvedValueOnce({ chat_input_actions_scale: 137 });
    const { useSettingsStore } = await import('../settingsStore');

    await useSettingsStore.getState().fetchSettings();

    expect(useSettingsStore.getState().settings.chat_input_actions_scale).toBe(140);
  });

  it('normalizes an imported scale before persistence', async () => {
    invokeMock.mockResolvedValueOnce({}).mockResolvedValueOnce(undefined);
    const { useSettingsStore } = await import('../settingsStore');
    await useSettingsStore.getState().fetchSettings();

    await useSettingsStore.getState().saveSettings({ chat_input_actions_scale: 44 });

    expect(useSettingsStore.getState().settings.chat_input_actions_scale).toBe(50);
    expect(invokeMock).toHaveBeenLastCalledWith(
      'save_settings',
      expect.objectContaining({
        settings: expect.objectContaining({ chat_input_actions_scale: 50 }),
      }),
    );
  });

  it('loads settings before saving when the store was reset', async () => {
    invokeMock
      .mockResolvedValueOnce({ chat_input_actions_scale: 100 })
      .mockResolvedValueOnce(undefined);
    const { useSettingsStore } = await import('../settingsStore');

    await useSettingsStore.getState().saveSettings({ chat_input_actions_scale: 50 });

    expect(useSettingsStore.getState().settings.chat_input_actions_scale).toBe(50);
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'get_settings');
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      'save_settings',
      expect.objectContaining({
        settings: expect.objectContaining({ chat_input_actions_scale: 50 }),
      }),
    );
  });
});
