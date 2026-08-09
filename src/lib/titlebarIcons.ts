import type { AppSettings, TitlebarIconId, TitlebarToggleableIconId } from '@/types';

export const TITLEBAR_TOGGLEABLE_ICONS: TitlebarToggleableIconId[] = [
  'pin',
  'theme',
  'language',
  'backup',
  'github',
  'update',
  'reload',
];

export const TITLEBAR_ICON_IDS: TitlebarIconId[] = [
  ...TITLEBAR_TOGGLEABLE_ICONS,
  'settings',
];

export const TITLEBAR_ICON_LABEL_KEYS: Record<TitlebarIconId, string> = {
  pin: 'settings.titlebarIcon.pin',
  theme: 'settings.titlebarIcon.theme',
  language: 'settings.titlebarIcon.language',
  backup: 'settings.titlebarIcon.backup',
  github: 'settings.titlebarIcon.github',
  update: 'settings.titlebarIcon.update',
  reload: 'settings.titlebarIcon.reload',
  settings: 'settings.titlebarIcon.settings',
};

export function isTitlebarIconVisible(
  settings: Pick<AppSettings, 'titlebar_icon_visibility'>,
  id: TitlebarToggleableIconId,
): boolean {
  return settings.titlebar_icon_visibility?.[id] !== false;
}
