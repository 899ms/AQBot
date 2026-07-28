import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
}));

describe('settings sidebar density persistence', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it('uses the standard density when legacy settings omit the field', async () => {
    invokeMock.mockResolvedValueOnce({ font_size: 18 });
    const { useSettingsStore } = await import('../settingsStore');

    await useSettingsStore.getState().fetchSettings();

    expect(useSettingsStore.getState().settings.settings_sidebar_density).toBe('standard');
  });

  it('optimistically updates and persists a density change', async () => {
    invokeMock.mockResolvedValueOnce({}).mockResolvedValueOnce(undefined);
    const { useSettingsStore } = await import('../settingsStore');
    await useSettingsStore.getState().fetchSettings();

    await useSettingsStore.getState().saveSettings({
      settings_sidebar_density: 'spacious',
    });

    expect(useSettingsStore.getState().settings.settings_sidebar_density).toBe('spacious');
    expect(invokeMock).toHaveBeenLastCalledWith(
      'save_settings',
      expect.objectContaining({
        settings: expect.objectContaining({
          settings_sidebar_density: 'spacious',
        }),
      }),
    );
  });
});
