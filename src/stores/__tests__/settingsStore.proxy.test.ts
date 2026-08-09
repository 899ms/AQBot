import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@/lib/invoke', () => ({
  invoke: invokeMock,
}));

describe('proxy settings defaults', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it('uses the system proxy when persisted settings omit the field', async () => {
    invokeMock.mockResolvedValueOnce({ font_size: 18 });
    const { useSettingsStore } = await import('../settingsStore');

    await useSettingsStore.getState().fetchSettings();

    expect(useSettingsStore.getState().settings.proxy_type).toBe('system');
  });

  it('preserves an explicitly disabled proxy', async () => {
    invokeMock.mockResolvedValueOnce({ proxy_type: null });
    const { useSettingsStore } = await import('../settingsStore');

    await useSettingsStore.getState().fetchSettings();

    expect(useSettingsStore.getState().settings.proxy_type).toBeNull();
  });
});
