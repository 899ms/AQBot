import { useEffect, useMemo, useState } from 'react';
import { Button, Modal, theme } from 'antd';
import { Eye, FileText, Trash2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { PastedSnippet } from '@/lib/pastedText';

function fileExtensionBadge(fileName: string): string {
  const ext = fileName.split('.').pop()?.toUpperCase() || 'FILE';
  return ext.length > 5 ? ext.slice(0, 5) : ext;
}

function isImageFile(file: File): boolean {
  return file.type.startsWith('image/') || /\.(png|jpe?g|gif|webp|bmp|svg|ico)$/i.test(file.name);
}

type FileChipProps = {
  file: File;
  onRemove: () => void;
};

function FileChip({ file, onRemove }: FileChipProps) {
  const { token } = theme.useToken();
  const isImage = isImageFile(file);
  const [thumbUrl, setThumbUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!isImage) return;
    const url = URL.createObjectURL(file);
    setThumbUrl(url);
    return () => {
      URL.revokeObjectURL(url);
    };
  }, [file, isImage]);

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
      {isImage && thumbUrl ? (
        <img
          src={thumbUrl}
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
        aria-label="remove-attachment"
      />
    </span>
  );
}

type SnippetBarProps = {
  snippet: PastedSnippet;
  onPreview: () => void;
  onExpand: () => void;
  onRemove: () => void;
};

function SnippetBar({ snippet, onPreview, onExpand, onRemove }: SnippetBarProps) {
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
        onClick={onExpand}
        aria-label={t('chat.expandPastedText')}
        style={{ color: token.colorTextSecondary, fontSize: 12, paddingInline: 6 }}
      >
        {t('chat.expandPastedText')}
      </Button>
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
}

export type AttachmentChipsProps = {
  files: File[];
  snippets: PastedSnippet[];
  onRemoveFile: (index: number) => void;
  onRemoveSnippet: (id: string) => void;
  onExpandSnippet: (id: string) => void;
};

export function AttachmentChips({
  files,
  snippets,
  onRemoveFile,
  onRemoveSnippet,
  onExpandSnippet,
}: AttachmentChipsProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [previewSnippet, setPreviewSnippet] = useState<PastedSnippet | null>(null);

  const hasContent = files.length > 0 || snippets.length > 0;
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
      {files.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {files.map((file, idx) => (
            <FileChip
              key={`${file.name}-${file.size}-${file.lastModified}-${idx}`}
              file={file}
              onRemove={() => onRemoveFile(idx)}
            />
          ))}
        </div>
      )}
      {snippets.map((snippet) => (
        <SnippetBar
          key={snippet.id}
          snippet={snippet}
          onPreview={() => setPreviewSnippet(snippet)}
          onExpand={() => onExpandSnippet(snippet.id)}
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
}
