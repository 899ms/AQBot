import type React from 'react';
import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSettings } from '@/types';
import {
  resolveSettingsSidebarItemHeight,
  SettingsSidebar,
} from '../SettingsSidebar';

let settings: Pick<AppSettings, 'font_size' | 'settings_sidebar_density'>;

const mocks = vi.hoisted(() => ({
  setSettingsSection: vi.fn(),
  exitSettings: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string | string[]) => (Array.isArray(key) ? key[key.length - 1] : key),
  }),
}));

vi.mock('antd', () => ({
  Menu: ({
    items,
    styles,
    onClick,
  }: {
    items: Array<{ key: string; label: React.ReactNode }>;
    styles?: { item?: React.CSSProperties };
    onClick?: (info: { key: string }) => void;
  }) => (
    <nav
      aria-label="settings-menu"
      data-height={styles?.item?.height}
      data-line-height={styles?.item?.lineHeight}
    >
      {items.map((item) => (
        <button key={item.key} type="button" onClick={() => onClick?.({ key: item.key })}>
          {item.label}
        </button>
      ))}
    </nav>
  ),
  theme: {
    useToken: () => ({
      token: {
        colorBgContainer: '#ffffff',
        colorBorderSecondary: '#eeeeee',
        colorFillSecondary: '#f5f5f5',
        colorText: '#111111',
        colorTextQuaternary: '#999999',
        colorTextSecondary: '#444444',
      },
    }),
  },
}));

vi.mock('@/stores', () => ({
  useSettingsStore: (selector: (state: {
    settings: Pick<AppSettings, 'font_size' | 'settings_sidebar_density'>;
  }) => unknown) => selector({ settings }),
  useUIStore: (selector: (state: {
    settingsSection: string;
    setSettingsSection: typeof mocks.setSettingsSection;
    exitSettings: typeof mocks.exitSettings;
  }) => unknown) => selector({
    settingsSection: 'display',
    setSettingsSection: mocks.setSettingsSection,
    exitSettings: mocks.exitSettings,
  }),
}));

describe('resolveSettingsSidebarItemHeight', () => {
  it.each([
    [12, 'compact', 36],
    [14, 'standard', 40],
    [18, 'standard', 48],
    [20, 'standard', 52],
    [20, 'spacious', 56],
  ] as const)('maps %ipx and %s density to %ipx', (fontSize, density, expected) => {
    expect(resolveSettingsSidebarItemHeight(fontSize, density)).toBe(expected);
  });
});

describe('SettingsSidebar density', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    settings = {
      font_size: 14,
      settings_sidebar_density: 'standard',
    };
  });

  it('applies the resolved height only to this settings menu', () => {
    render(<SettingsSidebar />);

    const menu = screen.getByRole('navigation', { name: 'settings-menu' });
    expect(menu).toHaveAttribute('data-height', '40');
    expect(menu).toHaveAttribute('data-line-height', '40px');
  });

  it('recomputes the local menu height when font size and density change', () => {
    const { rerender } = render(<SettingsSidebar />);
    settings = {
      font_size: 20,
      settings_sidebar_density: 'spacious',
    };

    rerender(<SettingsSidebar />);

    expect(screen.getByRole('navigation', { name: 'settings-menu' })).toHaveAttribute(
      'data-height',
      '56',
    );
  });
});
