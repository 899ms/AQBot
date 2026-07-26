import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SelectionToolbarSettings } from '../SelectionToolbarSettings';

const mocks = vi.hoisted(() => {
  const runtime = {
    value: {
      state: 'permission_required',
      platform: 'macos',
      permission: 'denied',
      last_error: null,
      global_dismissal_supported: true,
    },
  };
  return {
    ensureProvidersLoaded: vi.fn(async () => {}),
    runtime,
    invoke: vi.fn(async (command: string) => command === 'selection_toolbar_open_permission_settings'
      ? {
          kind: 'manual_add_required',
          executable_path: '/workspace/target/debug/AQBot',
        }
      : runtime.value),
    saveSettings: vi.fn(async () => {}),
  };
});

beforeEach(() => {
  mocks.runtime.value = {
        state: 'permission_required',
        platform: 'macos',
        permission: 'denied',
        last_error: null,
        global_dismissal_supported: true,
  };
  mocks.invoke.mockClear();
});

vi.mock('@/lib/invoke', () => ({
  invoke: mocks.invoke,
}));

vi.mock('@/stores', () => ({
  useProviderStore: (selector: (state: Record<string, unknown>) => unknown) => selector({
    ensureProvidersLoaded: mocks.ensureProvidersLoaded,
  }),
  useSettingsStore: Object.assign(
    (selector: (state: Record<string, unknown>) => unknown) => selector({
      settings: {
        selection_toolbar: {
          enabled: false,
          theme_follow: false,
          tools: [
            {
              kind: 'builtin_action',
              builtin_key: 'copy',
              enabled: true,
            },
          ],
        },
      },
      saveSettings: mocks.saveSettings,
    }),
    {
      getState: () => ({
        error: null,
        settings: {
          selection_toolbar: {
            enabled: false,
            theme_follow: false,
            tools: [],
          },
        },
      }),
    },
  ),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/common/ModelParamSliders', () => ({
  ModelParamSliders: () => null,
}));

vi.mock('@/components/shared/ModelSelect', () => ({
  ModelSelect: () => null,
  parseModelValue: () => null,
}));

describe('SelectionToolbarSettings', () => {
  it('uses the full settings content width without a page-specific maximum', () => {
    render(<SelectionToolbarSettings />);

    const page = screen.getByTestId('selection-toolbar-settings');
    expect(page).toHaveStyle({ width: '100%' });
    expect(page.style.maxWidth).toBe('');
  });

  it('explains how to add an unbundled macOS development executable', async () => {
    const user = userEvent.setup();
    render(<SelectionToolbarSettings />);

    await user.click(await screen.findByRole('button', {
      name: 'settings.selectionToolbar.openPermission',
    }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'settings.selectionToolbar.developmentPermissionHint',
    );
  });

  it('always shows denied permission and its authorization action independently of runtime state', async () => {
    mocks.runtime.value = {
      state: 'running',
      platform: 'macos',
      permission: 'denied',
      last_error: null,
      global_dismissal_supported: true,
    };

    render(<SelectionToolbarSettings />);

    expect(await screen.findByText(
      'settings.selectionToolbar.permission.denied',
    )).toBeInTheDocument();
    expect(screen.getByRole('button', {
      name: 'settings.selectionToolbar.openPermission',
    })).toBeInTheDocument();
    expect(screen.getByRole('button', {
      name: 'settings.selectionToolbar.requestPermission',
    })).toBeInTheDocument();
    expect(screen.getByText(
      'settings.selectionToolbar.permissionDeniedHint',
    )).toBeInTheDocument();
  });

  it('opens a guided authorization flow and the macOS permission pane together', async () => {
    const user = userEvent.setup();
    render(<SelectionToolbarSettings />);

    await user.click(await screen.findByRole('button', {
      name: 'settings.selectionToolbar.requestPermission',
    }));

    expect(await screen.findByRole('dialog')).toHaveTextContent(
      'settings.selectionToolbar.guideTitle',
    );
    expect(screen.getByText(
      'settings.selectionToolbar.guideStepEnable',
    )).toBeInTheDocument();
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith(
      'selection_toolbar_request_permission',
    ));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith(
      'selection_toolbar_open_permission_settings',
    ));
  });

  it('refreshes the permission status when the settings window regains focus', async () => {
    render(<SelectionToolbarSettings />);
    expect(await screen.findByText(
      'settings.selectionToolbar.permission.denied',
    )).toBeInTheDocument();

    mocks.runtime.value = {
      state: 'running',
      platform: 'macos',
      permission: 'granted',
      last_error: null,
      global_dismissal_supported: true,
    };
    fireEvent.focus(window);

    expect(await screen.findByText(
      'settings.selectionToolbar.permission.granted',
    )).toBeInTheDocument();
  });
});
