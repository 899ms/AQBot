import { memo, useMemo, useState } from 'react';
import { Button, Modal, theme } from 'antd';
import { Eye, FileText, Trash2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { PastedSnippet } from '@/lib/pastedText';
import { isImageAttachmentFile } from './attachmentFileTypes';

function fileExtensionBadge(fileName: string): string {
  const ext = fileName.split('.').pop()?.toUpperCase() || 'FILE';
  return ext.length > 5 ? ext.slice(0, 5) : ext;
}

export function isImageFile(file: File): boolean {
  return isImageAttachmentFile(file);
}

/** Stable composer attachment: preview URL is created once on add and revoked on remove. */
export type ComposerAttachment = {
  id: string;
  file: File;
  /** Pre-created object URL for image previews; null for non-images. */
  previewUrl: string | null;
};

export function createComposerAttachment(file: File, id?: string): ComposerAttachment {
  const attachmentId = id ?? (typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `att-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`);
  return {
    id: attachmentId,
    file,
    previewUrl: isImageFile(file) ? URL.createObjectURL(file) : null,
  };
}

export function revokeComposerAttachment(attachment: ComposerAttachment): void {
  if (attachment.previewUrl) {
    URL.revokeObjectURL(attachment.previewUrl);
  }
}

export function revokeComposerAttachments(attachments: ComposerAttachment[]): void {
  for (const attachment of attachments) {
    revokeComposerAttachment(attachment);
  }
}

type FileChipProps = {
  attachment: ComposerAttachment;
  onRemove: () => void;
};

const FileChip = memo(function FileChip({ attachment, onRemove }: FileChipProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { file, previewUrl } = attachment;
  const isImage = previewUrl != null || isImageFile(file);

  return (
    <span
      className="inline-flex items-center gap-2 pr-1.5 text-xs"
      style={{
        backgroundColor: token.colorFillTertiary,
        borderRadius: token.borderRadiusLG,
        border: `1px solid ${token.colorBorderSecondary}`,
        maxWidth: 220,
        overflow: 'hidden',
      }}
    >
      {isImage && previewUrl ? (
        <img
          src={previewUrl}
          alt=""
          width={40}
          height={40}
          style={{
            width: 40,
            height: 40,
            objectFit: 'cover',
            display: 'block',
            flexShrink: 0,
          }}
        />
      ) : (
        <span
          className="inline-flex items-center justify-center"
          style={{
            width: 40,
            height: 40,
            backgroundColor: token.colorFillSecondary,
            color: token.colorTextSecondary,
            flexShrink: 0,
          }}
        >
          <FileText size={18} />
        </span>
      )}
      <span className="flex min-w-0 flex-col py-1.5 pr-1">
        <span
          style={{
            maxWidth: 140,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            color: token.colorText,
            fontWeight: 500,
          }}
          title={file.name}
        >
          {file.name}
        </span>
        <span style={{ color: token.colorTextTertiary, fontSize: 10, letterSpacing: 0.3 }}>
          {fileExtensionBadge(file.name)}
        </span>
      </span>
      <X
        size={14}
        className="cursor-pointer flex-shrink-0"
        style={{ color: token.colorTextTertiary }}
        onClick={onRemove}
        aria-label={t('common.removeAttachment')}
      />
    </span>
  );
});

type SnippetBarProps = {
  snippet: PastedSnippet;
  onPreview: () => void;
  onRemove: () => void;
};

const SnippetBar = memo(function SnippetBar({ snippet, onPreview, onRemove }: SnippetBarProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const label = t('chat.pastedTextLabel', {
    n: snippet.index,
    lines: snippet.lineCount,
  });

  return (
    <div
      className="flex w-full items-center gap-2 px-2.5 py-1.5 text-xs"
      style={{
        backgroundColor: token.colorFillTertiary,
        borderRadius: token.borderRadius,
        border: `1px solid ${token.colorBorderSecondary}`,
        color: token.colorText,
      }}
    >
      <FileText size={14} style={{ color: token.colorTextSecondary, flexShrink: 0 }} />
      <span
        className="min-w-0 flex-1"
        style={{
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
        }}
        title={label}
      >
        [{label}]
      </span>
      <Button
        type="text"
        size="small"
        icon={<Eye size={13} />}
        onClick={onPreview}
        aria-label={t('chat.previewPastedText')}
        title={t('chat.previewPastedText')}
        style={{ color: token.colorTextSecondary }}
      />
      <Button
        type="text"
        size="small"
        icon={<Trash2 size={13} />}
        onClick={onRemove}
        aria-label={t('chat.removePastedText')}
        title={t('chat.removePastedText')}
        style={{ color: token.colorTextSecondary }}
      />
    </div>
  );
});

export type AttachmentChipsProps = {
  attachments: ComposerAttachment[];
  snippets: PastedSnippet[];
  onRemoveAttachment: (id: string) => void;
  onRemoveSnippet: (id: string) => void;
};

export const AttachmentChips = memo(function AttachmentChips({
  attachments,
  snippets,
  onRemoveAttachment,
  onRemoveSnippet,
}: AttachmentChipsProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [previewSnippet, setPreviewSnippet] = useState<PastedSnippet | null>(null);

  const hasContent = attachments.length > 0 || snippets.length > 0;
  const previewTitle = useMemo(() => {
    if (!previewSnippet) return '';
    return t('chat.pastedTextLabel', {
      n: previewSnippet.index,
      lines: previewSnippet.lineCount,
    });
  }, [previewSnippet, t]);

  if (!hasContent) return null;

  return (
    <div className="mb-2 flex flex-col gap-2">
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {attachments.map((attachment) => (
            <FileChip
              key={attachment.id}
              attachment={attachment}
              onRemove={() => onRemoveAttachment(attachment.id)}
            />
          ))}
        </div>
      )}
      {snippets.map((snippet) => (
        <SnippetBar
          key={snippet.id}
          snippet={snippet}
          onPreview={() => setPreviewSnippet(snippet)}
          onRemove={() => onRemoveSnippet(snippet.id)}
        />
      ))}

      <Modal
        open={!!previewSnippet}
        title={previewTitle}
        onCancel={() => setPreviewSnippet(null)}
        footer={null}
        width={720}
        destroyOnHidden
        styles={{
          body: {
            maxHeight: '60vh',
            overflow: 'auto',
            backgroundColor: token.colorBgLayout,
            borderRadius: token.borderRadius,
            padding: 12,
          },
        }}
      >
        <pre
          style={{
            margin: 0,
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            fontSize: 12,
            lineHeight: 1.5,
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
            color: token.colorText,
          }}
        >
          {previewSnippet?.content}
        </pre>
      </Modal>
    </div>
  );
});
