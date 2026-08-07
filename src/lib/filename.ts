/**
 * Sanitize a user-facing string for use as a file name segment.
 * Strips Windows/macOS/Linux illegal path characters and collapses whitespace.
 */
export function sanitizeFilenamePart(value: string, fallback = 'aqbot'): string {
  const normalized = value
    .trim()
    .replace(/[\\/:*?"<>|]+/g, '-')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
  return normalized || fallback;
}

/** Filesystem-safe local timestamp: `2026-03-27_143052` */
export function formatExportFilenameTimestamp(date: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return [
    date.getFullYear(),
    '-',
    pad(date.getMonth() + 1),
    '-',
    pad(date.getDate()),
    '_',
    pad(date.getHours()),
    pad(date.getMinutes()),
    pad(date.getSeconds()),
  ].join('');
}

/**
 * Build a safe download file name with extension and a unique timestamp suffix.
 * Example: `对话-计划 - 2026-03-27_143052.png`
 */
export function sanitizeExportFilename(
  title: string,
  extension: string,
  fallback = 'chat',
  at: Date = new Date(),
): string {
  const base = sanitizeFilenamePart(title, fallback);
  const ext = extension.replace(/^\./, '').toLowerCase() || 'bin';
  const timestamp = formatExportFilenameTimestamp(at);
  return `${base} - ${timestamp}.${ext}`;
}
