import { Popover, theme } from 'antd';
import { TriangleAlert } from 'lucide-react';
import type { ImageModelWarning } from '@/types';
import {
  getDrawingWarningDescription,
  getDrawingWarningTitle,
  type DrawingWarningTranslate,
} from '@/lib/drawingWarnings';

interface Props {
  warnings: ImageModelWarning[];
  modelId: string;
  translate: DrawingWarningTranslate;
}

/**
 * Chip-style button on the model label (icon + title). Detail text is hover-only
 * in a title-less popover.
 */
export function DrawingModelCompatibilityNotice({
  warnings,
  modelId,
  translate,
}: Props) {
  const { token } = theme.useToken();
  if (warnings.length === 0) return null;

  const title = String(
    translate('drawing.warning.compatibilityTitle', {}),
  );

  const content = (
    <div className="max-w-[240px] space-y-2">
      {warnings.map((warning) => {
        const body = getDrawingWarningTitle(warning, modelId, translate);
        const meta = getDrawingWarningDescription(warning, translate);
        return (
          <div key={warning.code} style={{ fontSize: 13, lineHeight: '20px' }}>
            <div>{body}</div>
            {meta && (
              <div style={{ marginTop: 4, color: token.colorTextSecondary }}>{meta}</div>
            )}
          </div>
        );
      })}
    </div>
  );

  return (
    <Popover
      content={content}
      trigger="hover"
      placement="topRight"
      mouseEnterDelay={0.15}
    >
      <button
        type="button"
        aria-label={title}
        className="inline-flex max-w-full shrink-0 items-center gap-1 border-0"
        style={{
          height: 22,
          padding: '0 8px',
          borderRadius: 6,
          background: token.colorWarningBg,
          color: token.colorWarningText,
          cursor: 'help',
          fontSize: 12,
          lineHeight: '20px',
          fontWeight: 500,
        }}
        onClick={(event) => event.preventDefault()}
      >
        <TriangleAlert size={12} strokeWidth={2.25} aria-hidden className="shrink-0" />
        <span className="truncate">{title}</span>
      </button>
    </Popover>
  );
}
