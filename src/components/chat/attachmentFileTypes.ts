const MIME_BY_EXTENSION: Record<string, string> = {
  png: 'image/png',
  apng: 'image/apng',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  jfif: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  avif: 'image/avif',
  heic: 'image/heic',
  heif: 'image/heif',
  tif: 'image/tiff',
  tiff: 'image/tiff',
  jxl: 'image/jxl',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  pdf: 'application/pdf',
  doc: 'application/msword',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  txt: 'text/plain',
  md: 'text/markdown',
  markdown: 'text/markdown',
  csv: 'text/csv',
  json: 'application/json',
  html: 'text/html',
  htm: 'text/html',
  xml: 'application/xml',
  js: 'text/javascript',
  jsx: 'text/jsx',
  ts: 'text/typescript',
  tsx: 'text/tsx',
  css: 'text/css',
  yaml: 'application/yaml',
  yml: 'application/yaml',
  toml: 'application/toml',
  zip: 'application/zip',
  gz: 'application/gzip',
  tar: 'application/x-tar',
};

export function getAttachmentMimeType(fileName: string, mimeType?: string): string {
  const normalizedMimeType = mimeType?.trim();
  const normalizedMimeTypeLower = normalizedMimeType?.toLowerCase();
  const extension = fileName.split('.').pop()?.toLowerCase() || '';
  const extensionMimeType = MIME_BY_EXTENSION[extension];
  if (normalizedMimeTypeLower?.startsWith('image/')) {
    return normalizedMimeTypeLower;
  }
  if (extensionMimeType?.startsWith('image/')) {
    return extensionMimeType;
  }
  if (
    normalizedMimeType
    && normalizedMimeTypeLower !== 'application/octet-stream'
  ) {
    return normalizedMimeType;
  }
  return extensionMimeType || normalizedMimeType || 'application/octet-stream';
}

export function isImageAttachmentFile(file: Pick<File, 'name' | 'type'>): boolean {
  return getAttachmentMimeType(file.name, file.type).toLowerCase().startsWith('image/');
}
