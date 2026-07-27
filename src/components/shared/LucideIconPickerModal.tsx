import { Input, Modal, theme } from 'antd';
import { Search } from 'lucide-react';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { lucideIconEntries } from '@/lib/lucideIconLibrary';

interface LucideIconPickerModalProps {
  open: boolean;
  onClose: () => void;
  onSelect: (icon: string) => void;
  value?: string;
}

/**
 * Grid picker over the full Lucide icon set (selection-toolbar custom tools).
 * Load via `lazy(() => import(...))` — this module drags in the whole icon
 * barrel through `lucideIconLibrary`.
 */
export default function LucideIconPickerModal({
  open,
  onClose,
  onSelect,
  value,
}: LucideIconPickerModalProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [search, setSearch] = useState('');
  const entries = useMemo(() => lucideIconEntries(), []);

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return entries;
    return entries.filter((entry) => entry.name.includes(query));
  }, [entries, search]);

  const close = () => {
    onClose();
    setSearch('');
  };

  return (
    <Modal
      destroyOnHidden
      footer={null}
      mask={{ enabled: true, blur: true }}
      open={open}
      title={t('settings.chooseIcon')}
      width={520}
      onCancel={close}
    >
      <Input
        allowClear
        autoFocus
        className="mb-3"
        placeholder={t('settings.searchIcon')}
        prefix={<Search size={14} />}
        value={search}
        onChange={(event) => setSearch(event.target.value)}
      />
      <div
        className="grid grid-cols-6 gap-2 overflow-y-auto pr-1"
        data-os-scrollbar
        style={{ maxHeight: 360 }}
      >
        {filtered.map(({ name, Icon }) => (
          <button
            aria-pressed={name === value}
            className="icon-picker-item flex flex-col items-center gap-1 p-2 rounded-lg cursor-pointer transition-colors"
            key={name}
            style={{
              background: 'transparent',
              border: `1px solid ${name === value ? token.colorPrimary : token.colorBorderSecondary}`,
              // Skip layout/paint for offscreen rows — the full set is ~1600 icons.
              contentVisibility: 'auto',
              containIntrinsicSize: '62px',
            }}
            title={name}
            type="button"
            onClick={() => {
              onSelect(name);
              close();
            }}
          >
            <Icon size={22} />
            <span
              className="text-xs text-center truncate w-full"
              style={{ color: token.colorTextSecondary }}
            >
              {name}
            </span>
          </button>
        ))}
        {filtered.length === 0 && (
          <div className="col-span-6 py-8 text-center" style={{ color: token.colorTextQuaternary }}>
            {t('settings.selectionToolbar.iconPickerEmpty')}
          </div>
        )}
      </div>
      <style>{`
        .icon-picker-item:hover {
          background-color: ${token.colorPrimaryBg} !important;
          border-color: ${token.colorPrimary} !important;
        }
      `}</style>
    </Modal>
  );
}
