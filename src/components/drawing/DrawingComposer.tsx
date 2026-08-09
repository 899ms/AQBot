import { App, Button, Image, Tag, theme } from 'antd';
import { ArrowUp, GripHorizontal, X } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { ClipboardEvent as ReactClipboardEvent, PointerEvent as ReactPointerEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { loadStoredMediaSource } from '@/lib/storedMedia';
import { useDrawingStore } from '@/stores/drawingStore';
import { usePageTransientOpenState } from '@/components/layout/PageLifecycle';
import type {
  DrawingBackground,
  DrawingGenerateInput,
  DrawingImage,
  DrawingOutputFormat,
  DrawingQuality,
  DrawingSettings,
  ImageModelDescriptor,
  ImageOperation,
} from '@/types';

interface Props {
  settings: DrawingSettings;
  prompt: string;
  onPromptChange: (value: string) => void;
  onHeightChange?: (height: number) => void;
  supportedOperations?: ImageOperation[];
  targetDescriptor?: ImageModelDescriptor;
  targetAvailable?: boolean;
}

const TEXTAREA_MIN_HEIGHT = 72;
const TEXTAREA_MAX_HEIGHT = 260;
const PASTED_IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/jpg', 'image/webp']);
const DRAWING_QUALITIES = new Set<DrawingQuality>([
  'low',
  'medium',
  'high',
  'standard',
  'hd',
  'auto',
]);
const DRAWING_OUTPUT_FORMATS = new Set<DrawingOutputFormat>(['png', 'jpeg', 'webp']);
const DRAWING_BACKGROUNDS = new Set<DrawingBackground>(['auto', 'opaque', 'transparent']);

function normalizedDrawingValue<T extends string>(
  value: unknown,
  allowed: Set<T>,
  fallback: T,
): T {
  return typeof value === 'string' && allowed.has(value as T) ? value as T : fallback;
}

function clampTextareaHeight(value: number) {
  return Math.min(TEXTAREA_MAX_HEIGHT, Math.max(TEXTAREA_MIN_HEIGHT, value));
}

function getExtensionForImageType(type: string) {
  if (type === 'image/jpeg' || type === 'image/jpg') return 'jpg';
  if (type === 'image/webp') return 'webp';
  return 'png';
}

function ensurePastedImageName(file: File, index: number) {
  if (file.name) return file;
  const type = file.type || 'image/png';
  return new File(
    [file],
    `pasted-image-${Date.now()}-${index + 1}.${getExtensionForImageType(type)}`,
    { type, lastModified: file.lastModified },
  );
}

function getPastedImageFiles(items: DataTransferItemList | undefined) {
  if (!items) return [];

  const files: File[] = [];
  for (const item of items) {
    if (item.kind !== 'file' || !PASTED_IMAGE_TYPES.has(item.type)) continue;
    const file = item.getAsFile();
    if (file) files.push(ensurePastedImageName(file, files.length));
  }
  return files;
}

function DrawingEditPreview({ image, previewUrl }: { image: DrawingImage; previewUrl: string | null }) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [src, setSrc] = useState<string | null>(previewUrl);
  const [previewOpen, setPreviewOpen] = usePageTransientOpenState();

  useEffect(() => {
    if (previewUrl) {
      setSrc(previewUrl);
      return undefined;
    }

    let cancelled = false;
    setSrc(null);
    loadStoredMediaSource(image.stored_file_id, image.storage_path)
      .then((data) => { if (!cancelled) setSrc(data); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [image.storage_path, image.stored_file_id, previewUrl]);

  return (
    <div
      className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-md"
      style={{
        background: token.colorFillAlter,
        border: `1px solid ${token.colorBorderSecondary}`,
      }}
    >
      {src ? (
        <Image
          src={src}
          alt={t('drawing.editPreview')}
          width={36}
          height={36}
          style={{
            display: 'block',
            width: 36,
            height: 36,
            objectFit: 'cover',
            borderRadius: 6,
          }}
          preview={{
            open: previewOpen,
            onOpenChange: setPreviewOpen,
            mask: { blur: true },
            scaleStep: 0.5,
          }}
        />
      ) : null}
    </div>
  );
}

export function DrawingComposer({
  settings,
  prompt,
  onPromptChange,
  onHeightChange,
  supportedOperations,
  targetDescriptor,
  targetAvailable = true,
}: Props) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { message } = App.useApp();
  const references = useDrawingStore((s) => s.references);
  const editSourceImage = useDrawingStore((s) => s.editSourceImage);
  const editMaskFileId = useDrawingStore((s) => s.editMaskFileId);
  const editMaskFile = useDrawingStore((s) => s.editMaskFile);
  const editPreviewUrl = useDrawingStore((s) => s.editPreviewUrl);
  const selectImageForEdit = useDrawingStore((s) => s.selectImageForEdit);
  const uploadReferenceImage = useDrawingStore((s) => s.uploadReferenceImage);
  const generateImages = useDrawingStore((s) => s.generateImages);
  const editImage = useDrawingStore((s) => s.editImage);
  const editImageWithMask = useDrawingStore((s) => s.editImageWithMask);
  const submitting = useDrawingStore((s) => s.submitting);
  const [textareaHeight, setTextareaHeight] = useState(TEXTAREA_MIN_HEIGHT);
  const [resizing, setResizing] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const resizeStateRef = useRef<{ startY: number; startHeight: number } | null>(null);
  const requestedOperation: ImageOperation = editMaskFileId
    ? 'mask_edit'
    : editSourceImage || references.length > 0
      ? 'edit'
      : 'generate';
  const operationSupported = supportedOperations === undefined
    || supportedOperations.includes(requestedOperation);
  const submissionAvailable = targetAvailable && operationSupported;

  const handleResizeStart = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    resizeStateRef.current = {
      startY: event.clientY,
      startHeight: textareaHeight,
    };
    setResizing(true);
  }, [textareaHeight]);

  useEffect(() => {
    if (!resizing) return undefined;

    const handlePointerMove = (event: PointerEvent) => {
      const state = resizeStateRef.current;
      if (!state) return;
      setTextareaHeight(clampTextareaHeight(state.startHeight + state.startY - event.clientY));
    };

    const handlePointerUp = () => {
      resizeStateRef.current = null;
      setResizing(false);
    };

    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      resizeStateRef.current = null;
      setResizing(false);
    };
  }, [resizing]);

  useEffect(() => {
    const node = rootRef.current;
    if (!node || !onHeightChange) return undefined;

    const reportHeight = () => {
      const height = Math.ceil(node.getBoundingClientRect().height);
      if (height > 0) onHeightChange(height);
    };

    reportHeight();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(reportHeight);
    observer.observe(node);
    return () => observer.disconnect();
  }, [onHeightChange, textareaHeight, editSourceImage, editMaskFileId]);

  const handleSubmit = async () => {
    if (!submissionAvailable) {
      message.warning(t(
        'drawing.operationUnavailable',
      ));
      return;
    }
    if (!settings.providerId) {
      message.warning(t('drawing.selectProvider'));
      return;
    }
    const promptText = prompt.trim();
    if (!promptText) {
      message.warning(t('drawing.promptRequired'));
      return;
    }
    try {
      const targetKey = `${settings.providerId}::${settings.modelId}`;
      const storedParameters =
        settings.parametersByTarget?.[targetKey] ?? settings.parameters ?? {};
      const descriptorParameters = new Map(
        targetDescriptor?.parameters.map((parameter) => [parameter.key, parameter]) ?? [],
      );
      const parameterValue = (key: string, legacy: unknown, fallback: unknown) => {
        const descriptor = descriptorParameters.get(key);
        const candidate = storedParameters[key] ?? legacy ?? descriptor?.default ?? fallback;
        if (descriptor?.options.length && !descriptor.options.includes(candidate)) {
          return descriptor.default;
        }
        if (descriptor?.kind === 'number') {
          const number = Number(candidate);
          if (
            !Number.isFinite(number)
            || (descriptor.min !== null && number < descriptor.min)
            || (descriptor.max !== null && number > descriptor.max)
          ) {
            return descriptor.default;
          }
        }
        return candidate;
      };
      const supportsParameter = (key: string) =>
        targetDescriptor === undefined || descriptorParameters.has(key);
      const outputFormat = supportsParameter('output_format')
        ? normalizedDrawingValue(
          parameterValue('output_format', settings.outputFormat, 'png'),
          DRAWING_OUTPUT_FORMATS,
          'png',
        )
        : 'png';
      const storedCompression =
        storedParameters.output_compression ?? settings.outputCompression;
      const parameters = Object.fromEntries(
        Object.entries(storedParameters).filter(([key]) =>
          descriptorParameters.has(key)
          && ![
            'size',
            'quality',
            'output_format',
            'background',
            'output_compression',
            'n',
          ].includes(key),
        ),
      );
      const base: DrawingGenerateInput = {
        provider_id: settings.providerId,
        model_id: settings.modelId,
        prompt: promptText,
        size: supportsParameter('size')
          ? String(parameterValue('size', settings.size, 'auto'))
          : 'auto',
        quality: supportsParameter('quality')
          ? normalizedDrawingValue(
            parameterValue('quality', settings.quality, 'auto'),
            DRAWING_QUALITIES,
            'auto',
          )
          : 'auto',
        output_format: outputFormat,
        background: supportsParameter('background')
          ? normalizedDrawingValue(
            parameterValue('background', settings.background, 'auto'),
            DRAWING_BACKGROUNDS,
            'auto',
          )
          : 'auto',
        output_compression: supportsParameter('output_compression')
          && storedCompression !== undefined
          && storedCompression !== null
          ? Number(parameterValue('output_compression', storedCompression, 90))
          : undefined,
        reference_image_mode: settings.referenceImageMode,
        reference_image_format: settings.referenceImageFormat,
        reference_image_param_name: settings.referenceImageParamName,
        n: supportsParameter('n')
          ? Number(parameterValue('n', settings.n, 1))
          : 1,
        reference_file_ids: references.map((item) => item.id),
        generation_api_path: settings.generationApiPath,
        edit_api_path: settings.editApiPath,
        parameters,
      };
      onPromptChange('');
      if (editSourceImage && editMaskFileId) {
        await editImageWithMask({
          ...base,
          source_image_id: editSourceImage.id,
          mask_file_id: editMaskFile?.id ?? editMaskFileId,
        });
      } else if (editSourceImage) {
        await editImage({ ...base, source_image_id: editSourceImage.id });
      } else {
        await generateImages(base);
      }
    } catch (e) {
      message.error(String(e));
    }
  };

  const handlePaste = useCallback((event: ReactClipboardEvent<HTMLTextAreaElement>) => {
    const imageFiles = getPastedImageFiles(event.clipboardData?.items);
    if (imageFiles.length === 0) return;

    event.preventDefault();
    void (async () => {
      try {
        await Promise.all(imageFiles.map((file) => uploadReferenceImage(file)));
        message.success?.(t('drawing.referenceAdded'));
      } catch (error) {
        message.error?.(String(error));
      }
    })();
  }, [message, t, uploadReferenceImage]);

  return (
    <div
      ref={rootRef}
      className="absolute bottom-0 left-0 right-0 z-10 px-[10px] pb-5 pt-3"
      style={{
        backgroundColor: token.colorBgContainer,
      }}
    >
      <div
        data-testid="drawing-composer"
        style={{
          border: '1px solid var(--border-color)',
          borderRadius: 16,
          backgroundColor: token.colorBgContainer,
          overflow: 'hidden',
        }}
      >
        <div
          data-testid="drawing-composer-resize-handle"
          onPointerDown={handleResizeStart}
          style={{
            height: 12,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0,
            cursor: 'ns-resize',
            userSelect: 'none',
            touchAction: 'none',
          }}
        >
          <GripHorizontal size={14} style={{ color: token.colorTextQuaternary, opacity: 0.5 }} />
        </div>
        {editSourceImage && (
          <div className="flex items-center gap-2 px-3 pt-2 pb-1">
            <DrawingEditPreview image={editSourceImage} previewUrl={editPreviewUrl} />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <Tag color={editMaskFileId ? 'green' : 'blue'} style={{ width: 'fit-content', marginInlineEnd: 0 }}>
                {editMaskFileId ? t('drawing.maskEditMode') : t('drawing.editMode')}
              </Tag>
              <span className="min-w-0 truncate" style={{ fontSize: 12, color: token.colorTextSecondary }}>
                {editSourceImage.storage_path}
              </span>
            </div>
            <Button size="small" type="text" icon={<X size={14} />} onClick={() => selectImageForEdit(null)} />
          </div>
        )}
        <textarea
          className="aqbot-input-textarea"
          value={prompt}
          onChange={(event) => onPromptChange(event.target.value)}
          onPaste={handlePaste}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing || event.key === 'Process') return;
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault();
              handleSubmit();
            }
          }}
          placeholder={t('drawing.promptPlaceholder')}
          rows={2}
          style={{
            width: '100%',
            border: 'none',
            outline: 'none',
            resize: 'none',
            padding: '4px 16px 8px',
            fontSize: token.fontSize,
            lineHeight: 1.6,
            backgroundColor: 'transparent',
            color: token.colorText,
            fontFamily: 'inherit',
            minHeight: TEXTAREA_MIN_HEIGHT,
            height: textareaHeight,
            maxHeight: TEXTAREA_MAX_HEIGHT,
            overflowY: 'auto',
          }}
        />
        <div className="flex items-center justify-between px-2 pb-2">
          <div />
          <Button
            type="primary"
            shape="circle"
            size="small"
            icon={<ArrowUp size={14} />}
            loading={submitting}
            disabled={!prompt.trim() || !submissionAvailable}
            onClick={handleSubmit}
          />
        </div>
      </div>
    </div>
  );
}
