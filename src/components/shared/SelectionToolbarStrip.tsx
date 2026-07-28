import { Check, GripVertical, MoreHorizontal } from 'lucide-react';
import logo from '@/assets/image/logo.png';
import { SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS } from '@/types';
import { LucideToolIcon } from './LucideToolIcon';
import './selectionToolbarStrip.css';

export interface SelectionToolbarStripItem {
  id: string;
  icon: string;
  label: string;
  active?: boolean;
}

interface SelectionToolbarStripProps {
  items: SelectionToolbarStripItem[];
  busy?: boolean;
  copied?: boolean;
  preview?: boolean;
  previewLabel?: string;
  dragLabel: string;
  moreLabel: string;
  copiedLabel: string;
  onDragPointerDown?: () => void;
  onToolPointerDown?: (id: string) => void;
  onMorePointerDown?: () => void;
}

function hoverProps() {
  return {
    onMouseEnter: (event: React.MouseEvent<HTMLButtonElement>) => {
      event.currentTarget.dataset.hover = 'true';
    },
    onMouseLeave: (event: React.MouseEvent<HTMLButtonElement>) => {
      delete event.currentTarget.dataset.hover;
    },
  };
}

export function SelectionToolbarStrip({
  items,
  busy = false,
  copied = false,
  preview = false,
  previewLabel,
  dragLabel,
  moreLabel,
  copiedLabel,
  onDragPointerDown,
  onToolPointerDown,
  onMorePointerDown,
}: SelectionToolbarStripProps) {
  const visible = items.slice(0, SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS);
  const overflow = items.length > SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS;

  return (
    <div
      aria-label={preview ? previewLabel : undefined}
      className="selection-toolbar__bar"
      data-preview={preview ? 'true' : undefined}
      role={preview ? 'img' : undefined}
    >
      <button
        aria-label={dragLabel}
        className="selection-toolbar__drag"
        tabIndex={preview ? -1 : undefined}
        type="button"
        onPointerDown={(event) => {
          event.preventDefault();
          event.stopPropagation();
          if (preview || event.button !== 0) return;
          onDragPointerDown?.();
        }}
      >
        <GripVertical size={14} />
      </button>
      <img alt="" className="selection-toolbar__logo" draggable={false} src={logo} />
      <div className="selection-toolbar__tools">
        {visible.map((item) => (
          <button
            aria-label={item.label}
            aria-pressed={item.active}
            className="selection-toolbar__tool"
            data-active={item.active ? 'true' : undefined}
            disabled={busy}
            key={item.id}
            tabIndex={preview ? -1 : undefined}
            title={item.label}
            type="button"
            {...hoverProps()}
            onPointerDown={(event) => {
              event.preventDefault();
              event.stopPropagation();
              if (preview || event.button !== 0 || busy) return;
              onToolPointerDown?.(item.id);
            }}
          >
            <LucideToolIcon name={item.icon} size={14} />
            <span className="selection-toolbar__tool-label">{item.label}</span>
          </button>
        ))}
      </div>
      {copied && <Check aria-label={copiedLabel} className="selection-toolbar__copied" size={16} />}
      {overflow && (
        <button
          aria-label={moreLabel}
          className="selection-toolbar__more"
          disabled={busy}
          tabIndex={preview ? -1 : undefined}
          title={moreLabel}
          type="button"
          {...hoverProps()}
          onPointerDown={(event) => {
            event.preventDefault();
            event.stopPropagation();
            if (preview || event.button !== 0 || busy) return;
            onMorePointerDown?.();
          }}
        >
          <MoreHorizontal aria-hidden size={15} />
        </button>
      )}
    </div>
  );
}
