import React from 'react';
import { CloseCircleFilled } from '@ant-design/icons';
import { AlertCircle, FileImage, Paperclip } from 'lucide-react';
import { App, Dropdown, Image, Tag } from 'antd';
import { useTranslation } from 'react-i18next';
import {
  usePageConnectionGeneration,
  usePageTransientOpenState,
} from '@/components/layout/PageLifecycle';
import { invoke } from '@/lib/invoke';
import { loadStoredMediaSource } from '@/lib/storedMedia';
import type { Attachment } from '@/types';
import { isImageAttachmentFile } from './attachmentFileTypes';

const ATTACHMENT_IMG_STYLE: React.CSSProperties = {
  maxWidth: 200,
  maxHeight: 160,
  borderRadius: 8,
  objectFit: 'cover',
};

interface MessageAttachmentPreviewProps {
  attachment: Attachment;
  themeColor: string;
}

export function MessageAttachmentPreview({
  attachment,
  themeColor,
}: MessageAttachmentPreviewProps) {
  const { t } = useTranslation();
  const { modal, message } = App.useApp();
  const isImage = isImageAttachmentFile({
    name: attachment.file_name,
    type: attachment.file_type,
  });
  const [src, setSrc] = React.useState<string | null>(() => {
    if (!isImage || !attachment.data) return null;
    return `data:${attachment.file_type};base64,${attachment.data}`;
  });
  const [failed, setFailed] = React.useState(false);
  const [fileExists, setFileExists] = React.useState<boolean | null>(null);
  const [previewOpen, setPreviewOpen] = usePageTransientOpenState();
  const pageConnectionGenerationRef = usePageConnectionGeneration();

  React.useEffect(() => {
    if (isImage || fileExists !== null) return;
    if (!attachment.file_path) {
      setFileExists(false);
      return;
    }
    let cancelled = false;
    invoke<boolean>('check_attachment_exists', { filePath: attachment.file_path })
      .then((exists) => {
        if (!cancelled) setFileExists(exists);
      })
      .catch(() => {
        if (!cancelled) setFileExists(false);
      });
    return () => {
      cancelled = true;
    };
  }, [attachment.file_path, fileExists, isImage]);

  React.useEffect(() => {
    if (!isImage || src || failed) return;
    if (!attachment.file_path) {
      setFailed(true);
      setFileExists(false);
      return;
    }
    let cancelled = false;
    loadStoredMediaSource(attachment.id, attachment.file_path)
      .then((dataUrl) => {
        if (!cancelled) setSrc(dataUrl);
      })
      .catch(() => {
        if (!cancelled) {
          setFailed(true);
          setFileExists(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [attachment.file_path, attachment.id, failed, isImage, src]);

  if (fileExists === false) {
    const showMissingModal = () => {
      const connectionGeneration = pageConnectionGenerationRef.current;
      invoke<string>('resolve_attachment_path', { filePath: attachment.file_path })
        .then((absolutePath) => {
          if (pageConnectionGenerationRef.current !== connectionGeneration) return;
          modal.confirm({
            icon: <CloseCircleFilled style={{ color: '#ff4d4f' }} />,
            title: t('chat.attachmentNotFound'),
            content: absolutePath,
            okText: t('chat.attachmentOk'),
            cancelText: t('chat.attachmentRevealLocation'),
            onCancel: () => {
              void invoke('reveal_attachment_file', { filePath: attachment.file_path })
                .catch((error) => message.error(String(error)));
            },
          });
        })
        .catch(() => {
          if (pageConnectionGenerationRef.current !== connectionGeneration) return;
          modal.error({
            title: t('chat.attachmentNotFound'),
            content: attachment.file_path || attachment.file_name,
            okText: t('chat.attachmentOk'),
          });
        });
    };
    return (
      <Tag
        icon={<AlertCircle size={12} />}
        color="error"
        style={{ margin: 0, cursor: 'pointer' }}
        onClick={showMissingModal}
      >
        {attachment.file_name}
      </Tag>
    );
  }

  if (fileExists === null && !src) {
    return (
      <Tag
        icon={isImage ? <FileImage size={12} /> : <Paperclip size={12} />}
        style={{ margin: 0, cursor: 'default', opacity: 0.5 }}
      >
        {attachment.file_name}
      </Tag>
    );
  }

  if (isImage && src) {
    return (
      <Image
        src={src}
        alt={attachment.file_name}
        style={ATTACHMENT_IMG_STYLE}
        onError={() => {
          setFailed(true);
          setFileExists(false);
        }}
        preview={{
          open: previewOpen,
          onOpenChange: setPreviewOpen,
          mask: { blur: true },
          scaleStep: 0.5,
        }}
      />
    );
  }

  const handleOpen = () => {
    if (attachment.file_path) {
      void invoke('open_attachment_file', { filePath: attachment.file_path })
        .catch((error) => message.error(String(error)));
    }
  };
  const handleReveal = () => {
    if (attachment.file_path) {
      void invoke('reveal_attachment_file', { filePath: attachment.file_path })
        .catch((error) => message.error(String(error)));
    }
  };
  const tag = (
    <Tag
      icon={isImage ? <FileImage size={12} /> : <Paperclip size={12} />}
      color={themeColor}
      style={{ margin: 0, cursor: attachment.file_path ? 'pointer' : 'default' }}
      onClick={attachment.file_path ? handleOpen : undefined}
    >
      {attachment.file_name}
    </Tag>
  );

  if (!attachment.file_path) return tag;
  return (
    <Dropdown
      menu={{
        items: [
          { key: 'open', label: t('chat.attachmentOpen'), onClick: handleOpen },
          { key: 'reveal', label: t('chat.attachmentRevealInFinder'), onClick: handleReveal },
        ],
      }}
      trigger={['contextMenu']}
    >
      {tag}
    </Dropdown>
  );
}
