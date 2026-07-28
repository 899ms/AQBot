import {
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent,
} from 'react';
import { Check, GripVertical, MoreHorizontal } from 'lucide-react';
import logo from '@/assets/image/logo.png';
import {
  SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS,
  type SelectionToolbarDisplayMode,
  type SelectionToolbarOverflowDirection,
} from '@/types';
import { LucideToolIcon } from './LucideToolIcon';
import './selectionToolbarStrip.css';

const FULL_TOOLBAR_WIDTH = 320;
const FULL_TOOLBAR_CHROME_WIDTH = 53;
const FULL_TOOL_WIDTH_WITHOUT_LABEL = 28;
const TOOL_GAP = 2;
const MORE_WIDTH_WITH_GAP = 27;
const COMPACT_BASE_WIDTH = 52;
const COMPACT_TOOL_WIDTH = 30;
const COMPACT_MORE_WIDTH = 28;
const OVERFLOW_DROPDOWN_TOP = 38;
const OVERFLOW_DROPDOWN_CHROME_HEIGHT = 8;
const OVERFLOW_ITEM_HEIGHT = 28;
const OVERFLOW_DROPDOWN_MAX_HEIGHT = 176;

export interface SelectionToolbarStripItem {
  id: string;
  icon: string;
  label: string;
  active?: boolean;
}

interface SelectionToolbarStripProps {
  items: SelectionToolbarStripItem[];
  displayMode: SelectionToolbarDisplayMode;
  busy?: boolean;
  copied?: boolean;
  preview?: boolean;
  previewLabel?: string;
  expanded?: boolean;
  dropdownDirection?: SelectionToolbarOverflowDirection;
  dragLabel: string;
  moreLabel: string;
  copiedLabel: string;
  onDragPointerDown?: () => void;
  onToolPointerDown?: (id: string) => void;
  onMorePointerDown?: (overflowCount: number) => void;
  onVisibleCountChange?: (count: number) => void;
}

interface SelectionToolbarOverflowListProps {
  items: SelectionToolbarStripItem[];
  busy?: boolean;
  preview?: boolean;
  onToolPointerDown?: (id: string) => void;
}

function hoverProps() {
  return {
    onMouseEnter: (event: MouseEvent<HTMLButtonElement>) => {
      event.currentTarget.dataset.hover = 'true';
    },
    onMouseLeave: (event: MouseEvent<HTMLButtonElement>) => {
      delete event.currentTarget.dataset.hover;
    },
  };
}

export function compactSelectionToolbarWidth(toolCount: number): number {
  const visibleCount = Math.min(toolCount, SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS);
  return COMPACT_BASE_WIDTH
    + COMPACT_TOOL_WIDTH * visibleCount
    + (toolCount > visibleCount ? COMPACT_MORE_WIDTH : 0);
}

export function selectionToolbarOverflowSurfaceHeight(overflowCount: number): number {
  const dropdownHeight = Math.min(
    OVERFLOW_DROPDOWN_CHROME_HEIGHT + Math.max(1, overflowCount) * OVERFLOW_ITEM_HEIGHT,
    OVERFLOW_DROPDOWN_MAX_HEIGHT,
  );
  return OVERFLOW_DROPDOWN_TOP + dropdownHeight;
}

export function fullSelectionToolbarWidth(
  labelWidths: number[],
  visibleCount: number,
  reservedWidth = 0,
): number {
  const labelsWidth = labelWidths
    .slice(0, visibleCount)
    .reduce((sum, width) => sum + width, 0);
  const toolsWidth = visibleCount * FULL_TOOL_WIDTH_WITHOUT_LABEL
    + labelsWidth
    + Math.max(0, visibleCount - 1) * TOOL_GAP;
  const hasOverflow = visibleCount < labelWidths.length;
  return Math.min(
    FULL_TOOLBAR_WIDTH,
    Math.ceil(
      FULL_TOOLBAR_CHROME_WIDTH
      + toolsWidth
      + reservedWidth
      + (hasOverflow ? MORE_WIDTH_WITH_GAP : 0),
    ),
  );
}

export function fullSelectionToolbarVisibleCount(
  labelWidths: number[],
  reservedWidth = 0,
): number {
  const maximum = Math.min(labelWidths.length, SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS);
  for (let count = maximum; count >= 0; count -= 1) {
    const labelsWidth = labelWidths
      .slice(0, count)
      .reduce((sum, width) => sum + width, 0);
    const toolsWidth = count * FULL_TOOL_WIDTH_WITHOUT_LABEL
      + labelsWidth
      + Math.max(0, count - 1) * TOOL_GAP;
    const hasOverflow = count < labelWidths.length;
    const requiredWidth = FULL_TOOLBAR_CHROME_WIDTH
      + toolsWidth
      + reservedWidth
      + (hasOverflow ? MORE_WIDTH_WITH_GAP : 0);
    if (requiredWidth <= FULL_TOOLBAR_WIDTH) return count;
  }
  return 0;
}

export function SelectionToolbarStrip({
  items,
  displayMode,
  busy = false,
  copied = false,
  preview = false,
  previewLabel,
  expanded,
  dropdownDirection = 'below',
  dragLabel,
  moreLabel,
  copiedLabel,
  onDragPointerDown,
  onToolPointerDown,
  onMorePointerDown,
  onVisibleCountChange,
}: SelectionToolbarStripProps) {
  const initialVisibleCount = Math.min(items.length, SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS);
  const [measuredVisibleCount, setMeasuredVisibleCount] = useState(initialVisibleCount);
  const [fullWidth, setFullWidth] = useState(FULL_TOOLBAR_WIDTH);
  const labelNodes = useRef(new Map<string, HTMLSpanElement>());
  const compactVisibleCount = Math.min(items.length, SELECTION_TOOLBAR_MAX_VISIBLE_TOOLS);
  const compactToolsWidth = compactVisibleCount * FULL_TOOL_WIDTH_WITHOUT_LABEL
    + Math.max(0, compactVisibleCount - 1) * TOOL_GAP;
  const visibleCount = displayMode === 'compact'
    ? compactVisibleCount
    : measuredVisibleCount;

  useLayoutEffect(() => {
    if (displayMode === 'compact') {
      onVisibleCountChange?.(compactVisibleCount);
      return;
    }
    const labelWidths = items.map((item) =>
      labelNodes.current.get(item.id)?.getBoundingClientRect().width ?? 0);
    const next = fullSelectionToolbarVisibleCount(labelWidths, copied ? 27 : 0);
    setMeasuredVisibleCount((current) => current === next ? current : next);
    const nextWidth = fullSelectionToolbarWidth(labelWidths, next, copied ? 27 : 0);
    setFullWidth((current) => current === nextWidth ? current : nextWidth);
    onVisibleCountChange?.(next);
  }, [compactVisibleCount, copied, displayMode, items, onVisibleCountChange]);

  const visible = items.slice(0, visibleCount);
  const overflow = visibleCount < items.length;
  const width = displayMode === 'compact'
    ? compactSelectionToolbarWidth(items.length)
    : fullWidth;

  return (
    <div
      aria-label={preview ? previewLabel : undefined}
      className="selection-toolbar__bar"
      data-display-mode={displayMode}
      data-dropdown-direction={dropdownDirection}
      data-overflow={overflow ? 'true' : undefined}
      data-preview={preview ? 'true' : undefined}
      role={preview ? 'group' : undefined}
      style={{ width }}
    >
      <button
        aria-label={dragLabel}
        className="selection-toolbar__drag"
        tabIndex={preview ? -1 : undefined}
        title={dragLabel}
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
      <div
        className="selection-toolbar__tools"
        style={displayMode === 'compact'
          ? { flexBasis: compactToolsWidth, width: compactToolsWidth }
          : undefined}
      >
        {visible.map((item) => {
          const showCopiedIcon = displayMode === 'compact' && copied && item.id === 'copy';
          return (
            <button
              aria-label={item.label}
              aria-pressed={item.active}
              className="selection-toolbar__tool"
              data-active={item.active ? 'true' : undefined}
              disabled={busy}
              key={item.id}
              tabIndex={preview ? -1 : undefined}
              title={showCopiedIcon ? copiedLabel : item.label}
              type="button"
              {...hoverProps()}
              onPointerDown={(event) => {
                event.preventDefault();
                event.stopPropagation();
                if (preview || event.button !== 0 || busy) return;
                onToolPointerDown?.(item.id);
              }}
            >
              {showCopiedIcon
                ? <Check aria-hidden size={14} />
                : <LucideToolIcon name={item.icon} size={14} />}
              {displayMode === 'full' && (
                <span className="selection-toolbar__tool-label">{item.label}</span>
              )}
            </button>
          );
        })}
      </div>
      {displayMode === 'full' && copied && (
        <Check aria-label={copiedLabel} className="selection-toolbar__copied" size={16} />
      )}
      {overflow && (
        <div className="selection-toolbar__more-wrap">
          <button
            aria-expanded={expanded}
            aria-haspopup="menu"
            aria-label={moreLabel}
            className="selection-toolbar__more"
            disabled={busy}
            title={moreLabel}
            type="button"
            {...hoverProps()}
            onPointerDown={(event) => {
              event.preventDefault();
              event.stopPropagation();
              if (event.button !== 0 || busy) return;
              onMorePointerDown?.(items.length - visibleCount);
            }}
          >
            <MoreHorizontal aria-hidden size={15} />
          </button>
          {expanded && (
            <div
              aria-label={moreLabel}
              className="selection-toolbar__overflow-dropdown"
              role="menu"
            >
              <SelectionToolbarOverflowList
                busy={busy}
                items={items.slice(visibleCount)}
                preview={preview}
                onToolPointerDown={onToolPointerDown}
              />
            </div>
          )}
        </div>
      )}
      <div aria-hidden className="selection-toolbar__measure">
        {items.map((item) => (
          <span
            className="selection-toolbar__tool-label"
            key={item.id}
            ref={(node) => {
              if (node) labelNodes.current.set(item.id, node);
              else labelNodes.current.delete(item.id);
            }}
          >
            {item.label}
          </span>
        ))}
      </div>
    </div>
  );
}

export function SelectionToolbarOverflowList({
  items,
  busy = false,
  preview = false,
  onToolPointerDown,
}: SelectionToolbarOverflowListProps) {
  return (
    <div className="selection-toolbar__overflow-list">
      {items.map((item) => (
        <button
          aria-label={item.label}
          className="selection-toolbar__overflow-item"
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
          <span>{item.label}</span>
        </button>
      ))}
    </div>
  );
}
