export type DrawingParameterTranslate = (key: string, fallback: string) => string;

interface LocalizedLabel {
  key: string;
  fallback: string;
}

const PARAMETER_LABELS: Record<string, LocalizedLabel> = {
  aspect_ratio: { key: 'drawing.aspectRatio', fallback: 'Aspect ratio' },
  background: { key: 'drawing.background', fallback: 'Background' },
  guidance_scale: { key: 'drawing.guidanceScale', fallback: 'Guidance scale' },
  n: { key: 'drawing.batchCount', fallback: 'Batch count' },
  num_inference_steps: {
    key: 'drawing.inferenceSteps',
    fallback: 'Inference steps',
  },
  output_format: { key: 'drawing.outputFormat', fallback: 'Output format' },
  person_generation: {
    key: 'drawing.personGeneration',
    fallback: 'Person generation',
  },
  quality: { key: 'drawing.quality', fallback: 'Quality' },
  reference_image_format: {
    key: 'drawing.referenceImageFormat',
    fallback: 'Reference image format',
  },
  reference_image_mode: {
    key: 'drawing.referenceImageMode',
    fallback: 'Reference image transport',
  },
  resolution: { key: 'drawing.resolution', fallback: 'Resolution' },
  seed: { key: 'drawing.seed', fallback: 'Seed' },
  size: { key: 'drawing.size', fallback: 'Size' },
};

const PARAMETER_KEY_ALIASES: Record<string, string> = {
  batch_size: 'n',
  image_size: 'size',
  outputFormat: 'output_format',
  referenceImageFormat: 'reference_image_format',
  referenceImageMode: 'reference_image_mode',
};

const OPTION_LABELS: Record<string, Record<string, LocalizedLabel>> = {
  background: {
    opaque: { key: 'drawing.option.background.opaque', fallback: 'Opaque' },
    transparent: {
      key: 'drawing.option.background.transparent',
      fallback: 'Transparent',
    },
  },
  output_format: {
    png: { key: 'drawing.option.outputFormat.png', fallback: 'PNG' },
    jpeg: { key: 'drawing.option.outputFormat.jpeg', fallback: 'JPEG' },
    webp: { key: 'drawing.option.outputFormat.webp', fallback: 'WEBP' },
  },
  person_generation: {
    allow_adult: {
      key: 'drawing.option.personGeneration.allowAdult',
      fallback: 'Allow adults',
    },
    allow_all: {
      key: 'drawing.option.personGeneration.allowAll',
      fallback: 'Allow all',
    },
    dont_allow: {
      key: 'drawing.option.personGeneration.dontAllow',
      fallback: 'Do not allow',
    },
  },
  quality: {
    hd: { key: 'drawing.option.quality.hd', fallback: 'HD' },
    high: { key: 'drawing.option.quality.high', fallback: 'High' },
    low: { key: 'drawing.option.quality.low', fallback: 'Low' },
    medium: { key: 'drawing.option.quality.medium', fallback: 'Medium' },
    standard: { key: 'drawing.option.quality.standard', fallback: 'Standard' },
  },
  reference_image_format: {
    object: { key: 'drawing.option.referenceImageFormat.object', fallback: 'Object array' },
    string: { key: 'drawing.option.referenceImageFormat.string', fallback: 'String array' },
  },
  reference_image_mode: {
    base64: { key: 'drawing.option.referenceImageMode.base64', fallback: 'Base64' },
    multipart: {
      key: 'drawing.option.referenceImageMode.multipart',
      fallback: 'Multipart',
    },
  },
};

function canonicalParameterKey(parameterKey: string): string {
  return PARAMETER_KEY_ALIASES[parameterKey] ?? parameterKey;
}

export function getDrawingParameterLabel(
  parameterKey: string,
  t: DrawingParameterTranslate,
): string {
  const label = PARAMETER_LABELS[canonicalParameterKey(parameterKey)];
  return label ? t(label.key, label.fallback) : parameterKey;
}

export function getDrawingParameterValueLabel(
  parameterKey: string,
  value: unknown,
  translate: DrawingParameterTranslate,
): string {
  const canonicalKey = canonicalParameterKey(parameterKey);
  const stringValue = String(value);
  if (stringValue === 'auto') return translate('drawing.option.auto', 'Auto');
  const localized = OPTION_LABELS[canonicalKey]?.[stringValue];
  if (localized) return translate(localized.key, localized.fallback);
  return stringValue;
}

export function getDrawingParameterOption(
  parameterKey: string,
  value: unknown,
  t: DrawingParameterTranslate,
) {
  return {
    label: getDrawingParameterValueLabel(parameterKey, value, t),
    value,
  };
}

/**
 * AutoComplete fills the input with option `value`. Use the localized label as the
 * option value so dropdown + input show i18n text, then reverse-map on change.
 */
export function getDrawingParameterAutoCompleteOption(
  parameterKey: string,
  protocolValue: unknown,
  t: DrawingParameterTranslate,
) {
  const label = getDrawingParameterValueLabel(parameterKey, protocolValue, t);
  return {
    value: label,
    label,
  };
}

/**
 * Map an AutoComplete input (localized label or raw protocol value) back to the
 * protocol value expected by the image API. Free-form sizes pass through unchanged.
 */
export function resolveDrawingParameterProtocolValue(
  parameterKey: string,
  input: string,
  knownOptions: readonly unknown[],
  t: DrawingParameterTranslate,
): string {
  if (knownOptions.length === 0) return input;

  for (const option of knownOptions) {
    if (String(option) === input) return String(option);
  }
  for (const option of knownOptions) {
    if (getDrawingParameterValueLabel(parameterKey, option, t) === input) {
      return String(option);
    }
  }
  return input;
}
