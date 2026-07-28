import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import {
  SelectionToolbarStrip,
  compactSelectionToolbarWidth,
  fullSelectionToolbarVisibleCount,
  selectionToolbarOverflowSurfaceHeight,
} from '../SelectionToolbarStrip';

const items = Array.from({ length: 6 }, (_, index) => ({
  id: `tool-${index + 1}`,
  icon: 'sparkles',
  label: `Tool ${index + 1}`,
}));
const stripCss = readFileSync(
  resolve(process.cwd(), 'src/components/shared/selectionToolbarStrip.css'),
  'utf8',
);

function ruleBody(selector: string): string {
  const start = stripCss.indexOf(`${selector} {`);
  if (start < 0) return '';
  const bodyStart = stripCss.indexOf('{', start) + 1;
  return stripCss.slice(bodyStart, stripCss.indexOf('}', bodyStart));
}

describe('SelectionToolbarStrip', () => {
  it('moves full-mode tools into More until every visible label fits', () => {
    expect(fullSelectionToolbarVisibleCount([40, 40, 40, 40, 40, 40])).toBe(3);
    expect(fullSelectionToolbarVisibleCount([400])).toBe(0);
  });

  it('uses deterministic compact widths for visible icons and More', () => {
    expect(compactSelectionToolbarWidth(1)).toBe(82);
    expect(compactSelectionToolbarWidth(5)).toBe(202);
    expect(compactSelectionToolbarWidth(6)).toBe(230);
  });

  it('sizes the native overflow surface from the dropdown item count', () => {
    expect(selectionToolbarOverflowSurfaceHeight(1)).toBe(74);
    expect(selectionToolbarOverflowSurfaceHeight(2)).toBe(102);
    expect(selectionToolbarOverflowSurfaceHeight(6)).toBe(214);
    expect(selectionToolbarOverflowSurfaceHeight(20)).toBe(214);
  });

  it('renders compact tools without inline labels and retains title tooltips', () => {
    const { container } = render(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="compact"
        dragLabel="Drag"
        items={items}
        moreLabel="More"
      />,
    );

    const first = screen.getByRole('button', { name: 'Tool 1' });
    expect(first).not.toHaveTextContent('Tool 1');
    expect(first).toHaveAttribute('title', 'Tool 1');
    expect(screen.getByRole('button', { name: 'More' })).toBeInTheDocument();
    expect(container.querySelector('.selection-toolbar__bar')).toHaveStyle({ width: '230px' });
  });

  it('does not clip compact tool icons inside the tools container', () => {
    const compactToolsRule = ruleBody(
      ".selection-toolbar__bar[data-display-mode='compact'] .selection-toolbar__tools",
    );

    expect(compactToolsRule).toContain('overflow: visible;');
  });

  it('pins the compact tool group to its exact content width without centering offset', () => {
    const { container } = render(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="compact"
        dragLabel="Drag"
        items={items}
        moreLabel="More"
      />,
    );
    const compactBarRule = ruleBody(
      ".selection-toolbar__bar[data-display-mode='compact']",
    );

    expect(container.querySelector('.selection-toolbar__tools')).toHaveStyle({
      flexBasis: '148px',
      width: '148px',
    });
    expect(compactBarRule).toContain('justify-content: flex-start;');
  });

  it('restores five visible icons when switching from measured full mode to compact mode', () => {
    const rect = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect')
      .mockReturnValue({
        bottom: 0,
        height: 11,
        left: 0,
        right: 40,
        top: 0,
        width: 40,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      });
    const { container, rerender } = render(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="full"
        dragLabel="Drag"
        items={items}
        moreLabel="More"
      />,
    );

    expect(container.querySelectorAll('.selection-toolbar__tools > button')).toHaveLength(3);

    rerender(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="compact"
        dragLabel="Drag"
        items={items}
        moreLabel="More"
      />,
    );

    expect(container.querySelectorAll('.selection-toolbar__tools > button')).toHaveLength(5);
    expect(container.querySelector('.selection-toolbar__bar')).toHaveStyle({ width: '230px' });
    rect.mockRestore();
  });

  it('keeps More next to the tools and renders overflow as an anchored dropdown', () => {
    const { container } = render(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="full"
        dragLabel="Drag"
        expanded
        items={items}
        moreLabel="More"
      />,
    );

    const tools = container.querySelector('.selection-toolbar__tools');
    const moreWrap = container.querySelector('.selection-toolbar__more-wrap');
    const dropdown = screen.getByRole('menu', { name: 'More' });

    expect(tools?.nextElementSibling).toBe(moreWrap);
    expect(dropdown).toHaveClass('selection-toolbar__overflow-dropdown');
    expect(moreWrap).toContainElement(dropdown);
    expect(container.querySelector('.selection-toolbar__preview-overflow')).not.toBeInTheDocument();
  });

  it('matches overflow items to toolbar button sizing without a clipped shadow', () => {
    render(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="full"
        dragLabel="Drag"
        expanded
        items={items}
        moreLabel="More"
      />,
    );

    const dropdown = screen.getByRole('menu', { name: 'More' });
    const item = screen.getByRole('button', { name: 'Tool 6' });
    const icon = item.querySelector('svg');
    const dropdownRule = ruleBody('.selection-toolbar__overflow-dropdown');
    const itemRule = ruleBody('.selection-toolbar__overflow-item');
    const labelRule = ruleBody('.selection-toolbar__overflow-item > span');
    const darkDropdownRule = ruleBody('html[data-theme="dark"] .selection-toolbar__overflow-dropdown');

    expect(dropdown).toBeInTheDocument();
    expect(dropdownRule).toContain('box-shadow: none;');
    expect(darkDropdownRule).not.toContain('box-shadow:');
    expect(itemRule).toContain('height: 28px;');
    expect(itemRule).toContain('gap: 4px;');
    expect(itemRule).toContain('padding: 0 5px;');
    expect(labelRule).toContain('font-size: 11px;');
    expect(icon).toHaveAttribute('width', '14');
    expect(icon).toHaveAttribute('height', '14');
  });

  it('shrinks a full toolbar to its measured content instead of keeping empty side space', () => {
    const { container } = render(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="full"
        dragLabel="Drag"
        items={items}
        moreLabel="More"
      />,
    );

    expect(container.querySelector('.selection-toolbar__bar')).toHaveStyle({ width: '228px' });
  });

  it('anchors the dropdown above the bar when the native placement points upward', () => {
    const { container } = render(
      <SelectionToolbarStrip
        copiedLabel="Copied"
        displayMode="full"
        dragLabel="Drag"
        dropdownDirection="above"
        expanded
        items={items}
        moreLabel="More"
      />,
    );

    expect(container.querySelector('.selection-toolbar__bar'))
      .toHaveAttribute('data-dropdown-direction', 'above');
  });
});
