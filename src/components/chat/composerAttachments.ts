import { useCallback, useEffect, useRef, useState, type ChangeEvent, type ClipboardEvent, type DragEvent } from 'react';
import type { AttachmentInput } from '@/types';
import {
  createComposerAttachment,
  revokeComposerAttachment,
  revokeComposerAttachments,
  type ComposerAttachment,
} from './AttachmentChips';
import { getAttachmentMimeType, isImageAttachmentFile } from './attachmentFileTypes';

export { getAttachmentMimeType } from './attachmentFileTypes';

export const DOCUMENT_ATTACHMENT_ACCEPT = [
  '.pdf',
  '.doc',
  '.docx',
  '.txt',
  '.md',
  '.markdown',
  '.csv',
  '.json',
  '.html',
  '.htm',
  '.xml',
  'application/pdf',
  'application/msword',
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  'text/plain',
  'text/markdown',
  'text/csv',
  'application/json',
  'text/html',
  'text/xml',
  'application/xml',
].join(',');

const DOCUMENT_ATTACHMENT_MIME_TYPES = new Set([
  'application/pdf',
  'application/msword',
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  'text/plain',
  'text/markdown',
  'text/csv',
  'application/json',
  'text/html',
  'application/xml',
]);

export function isAllowedChatAttachmentFile(
  file: Pick<File, 'name' | 'type'>,
  hasVision: boolean,
  documentAttachmentReadingEnabled: boolean,
): boolean {
  const effectiveMimeType = getAttachmentMimeType(file.name, file.type);
  if (hasVision && isImageAttachmentFile(file)) return true;
  if (!documentAttachmentReadingEnabled) return false;
  return DOCUMENT_ATTACHMENT_MIME_TYPES.has(effectiveMimeType.toLowerCase());
}

export function isAllowedAcpAttachmentFile(
  file: Pick<File, 'name' | 'type'>,
  supportsImages: boolean,
): boolean {
  return supportsImages || !isImageAttachmentFile(file);
}

export async function fileToAttachmentInput(file: File): Promise<AttachmentInput> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error(`Failed to read ${file.name}`));
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== 'string') {
        reject(new Error(`Failed to read ${file.name}`));
        return;
      }
      resolve({
        file_name: file.name,
        file_type: getAttachmentMimeType(file.name, file.type),
        file_size: file.size,
        data: result.split(',')[1] || '',
      });
    };
    reader.readAsDataURL(file);
  });
}

function nativePathFileDescriptor(filePath: string): File {
  const fileName = filePath.split(/[\\/]/).pop() || 'file';
  return new File([], fileName, { type: getAttachmentMimeType(fileName), lastModified: 0 });
}

async function nativePathToFile(filePath: string, descriptor: File): Promise<File> {
  const { readFile, stat } = await import('@tauri-apps/plugin-fs');
  const [bytes, info] = await Promise.all([readFile(filePath), stat(filePath)]);
  return new File([bytes], descriptor.name, {
    type: descriptor.type,
    lastModified: info.mtime?.getTime() ?? 0,
  });
}

export interface UseComposerAttachmentsOptions {
  enabled?: boolean;
  acceptFile: (file: File) => boolean;
  onRejected?: (files: File[]) => void;
  onReadError?: (filePath: string, error: unknown) => void;
}

function attachmentDedupeKey(file: File): string {
  return `${file.name}:${file.size}:${getAttachmentMimeType(file.name, file.type).toLowerCase()}`;
}

export function useComposerAttachments({
  enabled = true,
  acceptFile,
  onRejected,
  onReadError,
}: UseComposerAttachmentsOptions) {
  const [attachments, setAttachments] = useState<ComposerAttachment[]>([]);
  const attachmentsRef = useRef(attachments);
  attachmentsRef.current = attachments;
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const htmlDragDepthRef = useRef(0);
  const connectedRef = useRef(false);

  useEffect(() => {
    connectedRef.current = true;
    return () => {
      connectedRef.current = false;
      revokeComposerAttachments(attachmentsRef.current);
      attachmentsRef.current = [];
      htmlDragDepthRef.current = 0;
      setAttachments([]);
      setIsDragging(false);
    };
  }, []);

  const addFiles = useCallback((incoming: File[]) => {
    if (!enabled || incoming.length === 0) return;
    const accepted = incoming.filter(acceptFile);
    const rejected = incoming.filter((file) => !acceptFile(file));
    if (rejected.length > 0) onRejected?.(rejected);
    if (accepted.length === 0) return;
    setAttachments((previous) => {
      const keys = new Set(
        previous.map(({ file }) => attachmentDedupeKey(file)),
      );
      const unique = accepted
        .filter((file) => {
          const key = attachmentDedupeKey(file);
          if (keys.has(key)) return false;
          keys.add(key);
          return true;
        })
        .map((file) => createComposerAttachment(file));
      return unique.length > 0 ? [...previous, ...unique] : previous;
    });
  }, [acceptFile, enabled, onRejected]);

  const removeAttachment = useCallback((id: string) => {
    setAttachments((previous) => {
      const target = previous.find((item) => item.id === id);
      if (target) revokeComposerAttachment(target);
      return previous.filter((item) => item.id !== id);
    });
  }, []);

  const resetAttachments = useCallback(() => {
    revokeComposerAttachments(attachmentsRef.current);
    attachmentsRef.current = [];
    htmlDragDepthRef.current = 0;
    setIsDragging(false);
    setAttachments([]);
  }, []);

  const detachAttachments = useCallback(() => {
    const detached = attachmentsRef.current;
    attachmentsRef.current = [];
    setAttachments([]);
    return detached;
  }, []);

  const restoreAttachments = useCallback((items: ComposerAttachment[]) => {
    if (!connectedRef.current) {
      revokeComposerAttachments(items);
      return;
    }
    setAttachments((current) => {
      const currentKeys = new Set(
        current.map(({ file }) => attachmentDedupeKey(file)),
      );
      const restored = items.filter((item) => {
        if (current.includes(item)) return false;
        const key = attachmentDedupeKey(item.file);
        if (!currentKeys.has(key)) return true;
        revokeComposerAttachment(item);
        return false;
      });
      const next = restored.length > 0 ? [...restored, ...current] : current;
      attachmentsRef.current = next;
      return next;
    });
  }, []);

  const openFilePicker = useCallback(() => fileInputRef.current?.click(), []);

  const handleFileChange = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    addFiles(Array.from(event.target.files ?? []));
    event.target.value = '';
  }, [addFiles]);

  const handleClipboardFiles = useCallback((event: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = Array.from(event.clipboardData?.items ?? [])
      .filter((item) => item.kind === 'file')
      .map((item) => item.getAsFile())
      .filter((file): file is File => file !== null);
    if (files.length === 0) return false;
    if (!files.some(acceptFile)) {
      onRejected?.(files);
      return false;
    }
    event.preventDefault();
    addFiles(files);
    return true;
  }, [acceptFile, addFiles, onRejected]);

  useEffect(() => {
    if (!enabled || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent(async (event) => {
        if (cancelled) return;
        if (event.payload.type === 'enter') {
          setIsDragging(true);
          return;
        }
        if (event.payload.type === 'leave') {
          htmlDragDepthRef.current = 0;
          setIsDragging(false);
          return;
        }
        if (event.payload.type !== 'drop') return;
        htmlDragDepthRef.current = 0;
        setIsDragging(false);
        const files: File[] = [];
        const rejected: File[] = [];
        for (const filePath of event.payload.paths) {
          const descriptor = nativePathFileDescriptor(filePath);
          if (!acceptFile(descriptor)) {
            rejected.push(descriptor);
            continue;
          }
          try {
            files.push(await nativePathToFile(filePath, descriptor));
          } catch (error) {
            onReadError?.(filePath, error);
          }
        }
        if (rejected.length > 0) onRejected?.(rejected);
        if (!cancelled) addFiles(files);
      }))
      .then((nextUnlisten) => {
        if (cancelled) nextUnlisten();
        else unlisten = nextUnlisten;
      })
      .catch((error) => onReadError?.('', error));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [addFiles, enabled, onReadError]);

  const handleDragEnter = useCallback((event: DragEvent) => {
    if (!enabled || !event.dataTransfer.types.includes('Files')) return;
    event.preventDefault();
    event.stopPropagation();
    htmlDragDepthRef.current += 1;
    setIsDragging(true);
  }, [enabled]);

  const handleDragOver = useCallback((event: DragEvent) => {
    if (!enabled || !event.dataTransfer.types.includes('Files')) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = 'copy';
  }, [enabled]);

  const handleDragLeave = useCallback((event: DragEvent) => {
    if (!enabled) return;
    event.preventDefault();
    event.stopPropagation();
    htmlDragDepthRef.current = Math.max(0, htmlDragDepthRef.current - 1);
    if (htmlDragDepthRef.current === 0) setIsDragging(false);
  }, [enabled]);

  const handleDrop = useCallback((event: DragEvent) => {
    if (!enabled) return;
    event.preventDefault();
    event.stopPropagation();
    htmlDragDepthRef.current = 0;
    setIsDragging(false);
    addFiles(Array.from(event.dataTransfer.files ?? []));
  }, [addFiles, enabled]);

  return {
    attachments,
    attachmentsRef,
    fileInputRef,
    isDragging,
    addFiles,
    removeAttachment,
    resetAttachments,
    detachAttachments,
    restoreAttachments,
    openFilePicker,
    handleFileChange,
    handleClipboardFiles,
    dragHandlers: {
      onDragEnter: handleDragEnter,
      onDragOver: handleDragOver,
      onDragLeave: handleDragLeave,
      onDrop: handleDrop,
    },
  };
}
