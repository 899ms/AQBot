import type { TFunction } from 'i18next';
import { setDefaultI18nMap } from 'markstream-react';

/**
 * Localize markstream's built-in UI labels (code block copy button, image
 * zoom controls, …). Shared by the main window and the selection toolbar so
 * both render markdown chrome identically.
 */
export function applyMarkstreamI18nMap(t: TFunction) {
  setDefaultI18nMap({
    'common.close': t('common.close'),
    'common.collapse': t('common.collapse'),
    'common.copied': t('common.copied'),
    'common.copy': t('common.copy'),
    'common.decrease': t('common.decrease'),
    'common.expand': t('common.expand'),
    'common.export': t('common.export'),
    'common.increase': t('common.increase'),
    'common.minimize': t('common.minimize'),
    'common.open': t('common.open'),
    'common.preview': t('common.preview'),
    'common.reset': t('common.reset'),
    'common.resetZoom': t('common.resetZoom'),
    'common.source': t('common.source'),
    'common.zoomIn': t('common.zoomIn'),
    'common.zoomOut': t('common.zoomOut'),
    'image.loadError': t('image.loadError'),
    'image.loading': t('image.loading'),
  });
}
