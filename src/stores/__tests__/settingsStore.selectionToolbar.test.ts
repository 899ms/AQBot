import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
}));

describe('selection toolbar settings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it('adds backward-compatible defaults when the backend has no toolbar settings', async () => {
    invokeMock.mockResolvedValueOnce({});
    const { useSettingsStore } = await import('../settingsStore');

    await useSettingsStore.getState().fetchSettings();

    expect(useSettingsStore.getState().settings.selection_toolbar).toMatchObject({
      enabled: false,
      theme_follow: false,
    });
    expect(useSettingsStore.getState().settings.selection_toolbar.tools).toHaveLength(4);
  });

  it('rolls back an optimistic toolbar update when persistence fails', async () => {
    invokeMock.mockResolvedValueOnce({}).mockRejectedValueOnce(new Error('save failed'));
    const { useSettingsStore } = await import('../settingsStore');
    await useSettingsStore.getState().fetchSettings();

    await useSettingsStore.getState().saveSettings({
      selection_toolbar: {
        ...useSettingsStore.getState().settings.selection_toolbar,
        enabled: true,
      },
    });

    expect(useSettingsStore.getState().settings.selection_toolbar.enabled).toBe(false);
    expect(useSettingsStore.getState().error).toContain('save failed');
  });
});
