import { isTauri } from '@/lib/invoke'
import { stripAqbotTags, safeParseChatMarkdown, type ChatMarkdownNode } from '@/lib/chatMarkdown'
import { sanitizeExportFilename } from '@/lib/filename'
import { formatChatTime } from '@/components/chat/chatTime'
import type { Message } from '@/types'

function browserDownload(filename: string, content: string, mimeType: string) {
  const blob = new Blob([content], { type: mimeType })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

async function saveFile(
  defaultName: string,
  content: string | Uint8Array,
  filters: { name: string; extensions: string[] }[],
) {
  if (isTauri()) {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const { writeTextFile, writeFile } = await import('@tauri-apps/plugin-fs')
    const filePath = await save({ defaultPath: defaultName, filters })
    if (!filePath) return false
    try {
      if (typeof content === 'string') {
        await writeTextFile(filePath, content)
      } else {
        await writeFile(filePath, content)
      }
    } catch (e) {
      console.error('Failed to write file:', filePath, e)
      throw e
    }
    return true
  }
  // Browser fallback
  const mimeType = filters[0]?.extensions[0] === 'png' ? 'image/png' : 'text/plain'
  if (typeof content === 'string') {
    browserDownload(defaultName, content, mimeType)
  } else {
    const blob = new Blob([content], { type: mimeType })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = defaultName
    a.click()
    URL.revokeObjectURL(url)
  }
  return true
}

async function writeToClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
    await writeText(text)
  }
}

export interface ExportRoleLabels {
  user: string;
  assistant: string;
  system: string;
}

export interface TranscriptExportOptions {
  includeThinking?: boolean;
  /** i18n role labels; defaults resolved from current language when omitted */
  roleLabels?: ExportRoleLabels;
  /** Profile display name for user messages (falls back to roleLabels.user) */
  userName?: string;
  /** Model display name for assistant messages (no provider prefix) */
  getModelLabel?: (message: Message) => string | undefined;
  formatTime?: (createdAt: number) => string;
}

export interface ExportPngTheme {
  colorPrimary: string;
  colorPrimaryBg: string;
  colorPrimaryBorder: string;
  colorFillSecondary?: string;
}

export interface ExportMessagesPngOptions extends TranscriptExportOptions {
  theme?: ExportPngTheme;
}

function getExportMessageContent(message: Message, options?: TranscriptExportOptions) {
  if (options?.includeThinking === false) {
    return stripAqbotTags(message.content, { stripThink: message.role !== 'user' })
  }
  return message.content
}

const FALLBACK_ROLE_LABELS: ExportRoleLabels = {
  user: 'You',
  assistant: 'Assistant',
  system: 'System',
}

function resolveRoleLabels(options?: TranscriptExportOptions): ExportRoleLabels {
  return {
    user: options?.roleLabels?.user || FALLBACK_ROLE_LABELS.user,
    assistant: options?.roleLabels?.assistant || FALLBACK_ROLE_LABELS.assistant,
    system: options?.roleLabels?.system || FALLBACK_ROLE_LABELS.system,
  }
}

/** Speaker line for export: user name / model name / system label (same as PNG). */
export function resolveExportSpeakerLabel(
  message: Message,
  options?: TranscriptExportOptions,
): string {
  const labels = resolveRoleLabels(options)
  if (message.role === 'user') {
    return options?.userName?.trim() || labels.user
  }
  if (message.role === 'system') {
    return labels.system
  }
  return options?.getModelLabel?.(message) || labels.assistant
}

function resolveExportMessageTime(
  message: Message,
  options?: TranscriptExportOptions,
): string | undefined {
  if (message.created_at == null) return undefined
  const formatTime = options?.formatTime ?? defaultFormatExportTime
  return formatTime(message.created_at)
}

export function buildMarkdownTranscript(messages: Message[], title: string, options?: TranscriptExportOptions) {
  const lines: string[] = [`# ${title}`, '']
  for (const m of messages) {
    const speaker = resolveExportSpeakerLabel(m, options)
    lines.push(`## ${speaker}`, '', getExportMessageContent(m, options), '')
    const time = resolveExportMessageTime(m, options)
    if (time) lines.push(time, '')
    lines.push('---', '')
  }
  return lines.join('\n')
}

export function buildTextTranscript(messages: Message[], title: string, options?: TranscriptExportOptions) {
  const lines: string[] = [title, '='.repeat(Math.max(title.length, 1)), '']
  for (const m of messages) {
    const speaker = resolveExportSpeakerLabel(m, options)
    lines.push(`[${speaker}]`, '', getExportMessageContent(m, options), '')
    const time = resolveExportMessageTime(m, options)
    if (time) lines.push(time, '')
    lines.push('---', '')
  }
  return lines.join('\n')
}

// ── Export markdown → safe HTML ──────────────────────────────────────

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function asRecord(node: ChatMarkdownNode): Record<string, unknown> {
  return node as unknown as Record<string, unknown>
}

function childrenOf(node: ChatMarkdownNode): ChatMarkdownNode[] {
  const children = asRecord(node).children
  return Array.isArray(children) ? children as ChatMarkdownNode[] : []
}

function renderInlineNodes(nodes: ChatMarkdownNode[]): string {
  return nodes.map(renderInlineNode).join('')
}

function renderInlineNode(node: ChatMarkdownNode): string {
  const rec = asRecord(node)
  switch (node.type) {
    case 'text':
      return escapeHtml(String(rec.content ?? rec.raw ?? ''))
    case 'strong':
      return `<strong>${renderInlineNodes(childrenOf(node))}</strong>`
    case 'emphasis':
      return `<em>${renderInlineNodes(childrenOf(node))}</em>`
    case 'strikethrough':
      return `<s>${renderInlineNodes(childrenOf(node))}</s>`
    case 'highlight':
      return `<mark>${renderInlineNodes(childrenOf(node))}</mark>`
    case 'insert':
      return `<u>${renderInlineNodes(childrenOf(node))}</u>`
    case 'subscript':
      return `<sub>${renderInlineNodes(childrenOf(node))}</sub>`
    case 'superscript':
      return `<sup>${renderInlineNodes(childrenOf(node))}</sup>`
    case 'inline_code':
      return `<code class="export-code-inline">${escapeHtml(String(rec.code ?? ''))}</code>`
    case 'link': {
      const href = String(rec.href ?? '')
      const safeHref = /^(https?:|mailto:)/i.test(href) ? escapeHtml(href) : '#'
      const text = childrenOf(node).length > 0
        ? renderInlineNodes(childrenOf(node))
        : escapeHtml(String(rec.text ?? href))
      return `<a href="${safeHref}">${text}</a>`
    }
    case 'image': {
      const alt = escapeHtml(String(rec.alt ?? ''))
      const src = String(rec.src ?? '')
      // Only embed data URLs / relative media in export; remote src may break html2canvas.
      if (/^(data:|https?:|http:\/\/aqbot-media\.localhost)/i.test(src)) {
        return `<img class="export-img" src="${escapeHtml(src)}" alt="${alt}" />`
      }
      return alt ? `<span class="export-img-fallback">[${alt}]</span>` : ''
    }
    case 'hardbreak':
      return '<br />'
    case 'checkbox':
    case 'checkbox_input':
      return rec.checked ? '☑ ' : '☐ '
    case 'emoji':
      return escapeHtml(String(rec.markup ?? rec.name ?? ''))
    case 'math_inline':
      return `<code class="export-math">${escapeHtml(String(rec.content ?? ''))}</code>`
    case 'footnote_reference':
      return `<sup>[${escapeHtml(String(rec.id ?? ''))}]</sup>`
    case 'html_inline':
      // Never inject raw HTML; fall back to text children or escaped content.
      if (childrenOf(node).length > 0) return renderInlineNodes(childrenOf(node))
      return escapeHtml(String(rec.content ?? ''))
    case 'inline':
      return renderInlineNodes(childrenOf(node))
    default:
      if (childrenOf(node).length > 0) return renderInlineNodes(childrenOf(node))
      if (typeof rec.content === 'string') return escapeHtml(rec.content)
      if (typeof rec.raw === 'string') return escapeHtml(rec.raw)
      return ''
  }
}

function renderBlockNode(node: ChatMarkdownNode): string {
  const rec = asRecord(node)
  switch (node.type) {
    case 'heading': {
      const level = Math.min(6, Math.max(1, Number(rec.level) || 1))
      const inner = childrenOf(node).length > 0
        ? renderInlineNodes(childrenOf(node))
        : escapeHtml(String(rec.text ?? ''))
      return `<h${level} class="export-h">${inner}</h${level}>`
    }
    case 'paragraph':
      return `<p class="export-p">${renderInlineNodes(childrenOf(node))}</p>`
    case 'blockquote':
      return `<blockquote class="export-quote">${childrenOf(node).map(renderBlockNode).join('')}</blockquote>`
    case 'list': {
      const ordered = Boolean(rec.ordered)
      const tag = ordered ? 'ol' : 'ul'
      const start = ordered && typeof rec.start === 'number' && rec.start !== 1
        ? ` start="${rec.start}"`
        : ''
      const items = Array.isArray(rec.items) ? rec.items as ChatMarkdownNode[] : []
      const lis = items.map((item) => {
        const itemChildren = childrenOf(item)
        // List items may contain paragraphs or inlines.
        const body = itemChildren.map((child) => {
          if (child.type === 'paragraph' || child.type === 'list' || child.type === 'blockquote' || child.type === 'code_block') {
            return renderBlockNode(child)
          }
          return renderInlineNode(child)
        }).join('')
        return `<li>${body}</li>`
      }).join('')
      return `<${tag} class="export-list"${start}>${lis}</${tag}>`
    }
    case 'code_block': {
      const lang = escapeHtml(String(rec.language ?? ''))
      const code = escapeHtml(String(rec.code ?? ''))
      const langLabel = lang ? `<div class="export-code-lang">${lang}</div>` : ''
      return `<div class="export-code-block">${langLabel}<pre><code>${code}</code></pre></div>`
    }
    case 'thematic_break':
      return '<hr class="export-hr" />'
    case 'table': {
      const header = rec.header as ChatMarkdownNode | undefined
      const rows = Array.isArray(rec.rows) ? rec.rows as ChatMarkdownNode[] : []
      const renderRow = (row: ChatMarkdownNode, isHeader: boolean) => {
        const cells = Array.isArray(asRecord(row).cells)
          ? asRecord(row).cells as ChatMarkdownNode[]
          : []
        const cellTag = isHeader ? 'th' : 'td'
        const tds = cells.map((cell) => {
          const align = asRecord(cell).align
          const alignAttr = align === 'left' || align === 'right' || align === 'center'
            ? ` style="text-align:${align}"`
            : ''
          return `<${cellTag}${alignAttr}>${renderInlineNodes(childrenOf(cell))}</${cellTag}>`
        }).join('')
        return `<tr>${tds}</tr>`
      }
      const thead = header ? `<thead>${renderRow(header, true)}</thead>` : ''
      const tbody = `<tbody>${rows.map((r) => renderRow(r, false)).join('')}</tbody>`
      return `<table class="export-table">${thead}${tbody}</table>`
    }
    case 'math_block':
      return `<pre class="export-math-block">${escapeHtml(String(rec.content ?? ''))}</pre>`
    case 'admonition': {
      const title = escapeHtml(String(rec.title || rec.kind || 'Note'))
      return `<div class="export-admonition"><div class="export-admonition-title">${title}</div>${childrenOf(node).map(renderBlockNode).join('')}</div>`
    }
    case 'html_block':
    case 'html-render':
      // Strip/escape untrusted HTML blocks.
      return `<pre class="export-html-fallback">${escapeHtml(String(rec.content ?? rec.raw ?? ''))}</pre>`
    case 'think':
    case 'web-search':
    case 'web-search-query':
    case 'knowledge-retrieval':
    case 'memory-retrieval':
    case 'tool-call':
      return '' // stripped for clean share images
    default: {
      // Custom components / unknown: try children, else plain escaped raw.
      if (childrenOf(node).length > 0) {
        return childrenOf(node).map((child) => {
          if (
            child.type === 'paragraph'
            || child.type === 'heading'
            || child.type === 'list'
            || child.type === 'code_block'
            || child.type === 'blockquote'
            || child.type === 'table'
          ) {
            return renderBlockNode(child)
          }
          return renderInlineNode(child)
        }).join('')
      }
      if (typeof rec.content === 'string' && rec.content.trim()) {
        return `<p class="export-p">${escapeHtml(rec.content)}</p>`
      }
      if (typeof rec.raw === 'string' && rec.raw.trim()) {
        return `<p class="export-p">${escapeHtml(rec.raw)}</p>`
      }
      return ''
    }
  }
}

/** Convert chat markdown to safe HTML for PNG export (XSS-escaped, no raw HTML passthrough). */
export function renderExportMarkdownHtml(content: string): string {
  const text = content.trim()
  if (!text) return ''
  try {
    const nodes = safeParseChatMarkdown(text)
    return nodes.map(renderBlockNode).join('')
  } catch (error) {
    console.error('Export markdown render failed, falling back to plain text:', error)
    return `<p class="export-p">${escapeHtml(text)}</p>`
  }
}

const EXPORT_MARKDOWN_CSS = `
.export-msg-name {
  font-size: 12px;
  font-weight: 600;
  color: #6b7280;
  line-height: 1.4;
  margin: 0 0 8px;
  padding: 0;
}
.export-md { font-size: 14px; line-height: 1.65; word-break: break-word; color: #111827; }
.export-md > :first-child { margin-top: 0; }
.export-md > :last-child { margin-bottom: 0; }
.export-h { margin: 12px 0 8px; font-weight: 600; line-height: 1.35; color: #111827; }
.export-h:first-child { margin-top: 0; }
h1.export-h { font-size: 1.35em; }
h2.export-h { font-size: 1.2em; }
h3.export-h { font-size: 1.08em; }
h4.export-h, h5.export-h, h6.export-h { font-size: 1em; }
.export-p { margin: 0 0 10px; white-space: pre-wrap; }
.export-p:last-child { margin-bottom: 0; }
.export-list { margin: 0 0 10px; padding-left: 1.4em; }
.export-list li { margin: 2px 0; }
.export-list li > .export-p { margin: 0; white-space: normal; }
.export-quote {
  margin: 0 0 10px;
  padding: 6px 12px;
  border-left: 3px solid #93c5fd;
  background: #f8fafc;
  color: #374151;
}
.export-code-inline {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.9em;
  background: #f3f4f6;
  border: 1px solid #e5e7eb;
  border-radius: 4px;
  padding: 0 4px;
}
.export-code-block {
  margin: 0 0 10px;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  overflow: hidden;
  background: #f9fafb;
}
.export-code-lang {
  font-size: 11px;
  color: #6b7280;
  padding: 4px 10px;
  border-bottom: 1px solid #e5e7eb;
  background: #f3f4f6;
}
.export-code-block pre {
  margin: 0;
  padding: 10px 12px;
  overflow-x: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12.5px;
  line-height: 1.5;
  white-space: pre;
}
.export-code-block code { font-family: inherit; background: none; border: none; padding: 0; }
.export-table {
  width: 100%;
  border-collapse: collapse;
  margin: 0 0 10px;
  font-size: 13px;
}
.export-table th, .export-table td {
  border: 1px solid #e5e7eb;
  padding: 6px 8px;
  text-align: left;
  vertical-align: top;
}
.export-table th { background: #f3f4f6; font-weight: 600; }
.export-hr { border: none; border-top: 1px solid #e5e7eb; margin: 12px 0; }
.export-math, .export-math-block {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.92em;
  background: #f8fafc;
}
.export-math-block { margin: 0 0 10px; padding: 8px 10px; border-radius: 6px; white-space: pre-wrap; }
.export-admonition {
  margin: 0 0 10px;
  padding: 8px 10px;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  background: #f9fafb;
}
.export-admonition-title { font-weight: 600; margin-bottom: 4px; font-size: 12px; color: #4b5563; }
.export-img { max-width: 100%; height: auto; border-radius: 6px; display: block; margin: 6px 0; }
.export-img-fallback { color: #6b7280; font-size: 12px; }
.export-html-fallback {
  margin: 0 0 10px;
  padding: 8px;
  background: #f9fafb;
  border-radius: 6px;
  font-size: 12px;
  white-space: pre-wrap;
  color: #4b5563;
}
.export-md a { color: #2563eb; text-decoration: underline; }
`

function injectExportMarkdownStyles(host: HTMLElement) {
  const style = document.createElement('style')
  style.textContent = EXPORT_MARKDOWN_CSS
  host.appendChild(style)
}

async function canvasToPngFile(canvas: HTMLCanvasElement, title: string) {
  const safeName = sanitizeExportFilename(title, 'png', 'chat')
  if (isTauri()) {
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
    if (!blob) return false
    const buffer = new Uint8Array(await blob.arrayBuffer())
    return saveFile(safeName, buffer, [{ name: 'PNG Image', extensions: ['png'] }])
  }

  // Browser fallback
  const link = document.createElement('a')
  link.download = safeName
  link.href = canvas.toDataURL('image/png')
  link.click()
  return true
}

/** Expand overflow/scroll containers and hide interactive chrome before capture. */
function prepareClonedExportRoot(cloned: HTMLElement) {
  cloned.style.height = 'auto'
  cloned.style.maxHeight = 'none'
  cloned.style.overflow = 'visible'
  cloned.style.position = 'static'

  cloned.querySelectorAll<HTMLElement>('*').forEach((node) => {
    const style = getComputedStyle(node)
    if (style.overflow === 'auto' || style.overflow === 'scroll' || style.overflowY === 'auto' || style.overflowY === 'scroll') {
      node.style.overflow = 'visible'
      node.style.overflowY = 'visible'
      node.style.height = 'auto'
      node.style.maxHeight = 'none'
    }
  })

  // Action bars / lucide toolbars render poorly in html2canvas and are noise in shares.
  cloned.querySelectorAll<HTMLElement>([
    '.ant-bubble-footer',
    '.ant-actions',
    '[class*="aqbot-action"]',
    '[data-export-hide="true"]',
  ].join(',')).forEach((node) => {
    node.style.display = 'none'
  })
}

export async function exportAsPNG(element: HTMLElement | null, title: string) {
  if (!element) return false
  const { default: html2canvas } = await import('html2canvas')
  const canvas = await html2canvas(element, {
    useCORS: true,
    scale: 2,
    backgroundColor: '#fff',
    scrollX: 0,
    scrollY: -window.scrollY,
    windowWidth: Math.max(element.scrollWidth, element.clientWidth),
    windowHeight: Math.max(element.scrollHeight, element.clientHeight),
    height: Math.max(element.scrollHeight, element.clientHeight),
    width: Math.max(element.scrollWidth, element.clientWidth),
    onclone: (_document, clonedElement) => {
      prepareClonedExportRoot(clonedElement)
    },
  })

  return canvasToPngFile(canvas, title)
}

async function resolveDefaultRoleLabels(): Promise<ExportRoleLabels> {
  try {
    const i18n = (await import('@/i18n')).default
    return {
      user: i18n.t('chat.you'),
      assistant: i18n.t('chat.assistant'),
      system: i18n.t('chat.system'),
    }
  } catch {
    return { ...FALLBACK_ROLE_LABELS }
  }
}

function defaultFormatExportTime(createdAt: number): string {
  return formatChatTime(createdAt)
}

/**
 * Render selected messages into a clean off-screen card layout, then capture PNG.
 * No avatars — role line is user name / model name / system label; footer is time only.
 */
export async function exportMessagesAsPNG(
  messages: Message[],
  title: string,
  options?: ExportMessagesPngOptions,
) {
  if (messages.length === 0) return false

  const roleLabels = options?.roleLabels ?? await resolveDefaultRoleLabels()
  const theme: ExportPngTheme = {
    colorPrimary: options?.theme?.colorPrimary ?? '#1677ff',
    colorPrimaryBg: options?.theme?.colorPrimaryBg ?? '#e6f4ff',
    colorPrimaryBorder: options?.theme?.colorPrimaryBorder ?? '#91caff',
    colorFillSecondary: options?.theme?.colorFillSecondary ?? '#f3f4f6',
  }
  const userName = options?.userName?.trim() || roleLabels.user || 'You'
  const formatTime = options?.formatTime ?? defaultFormatExportTime

  const host = document.createElement('div')
  host.setAttribute('data-export-share-root', 'true')
  host.style.cssText = [
    'position:fixed',
    'left:-10000px',
    'top:0',
    'width:720px',
    'padding:28px 24px',
    'background:#ffffff',
    'color:#111827',
    'font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif',
    'box-sizing:border-box',
  ].join(';')

  injectExportMarkdownStyles(host)

  const heading = document.createElement('div')
  heading.style.cssText = 'font-size:18px;font-weight:600;margin:0 0 4px;line-height:1.4;'
  heading.textContent = title
  host.appendChild(heading)

  const meta = document.createElement('div')
  meta.style.cssText = 'font-size:12px;color:#6b7280;margin:0 0 20px;'
  meta.textContent = new Date().toLocaleString()
  host.appendChild(meta)

  for (const message of messages) {
    const isUser = message.role === 'user'
    const isSystem = message.role === 'system'
    const card = document.createElement('div')
    card.style.cssText = [
      'margin:0 0 14px',
      'padding:12px 14px',
      'border-radius:12px',
      `background:${isUser ? theme.colorPrimaryBg : '#f9fafb'}`,
      `border:1px solid ${isUser ? theme.colorPrimaryBorder : '#e5e7eb'}`,
    ].join(';')

    // Title: user name / model name / system label (no avatar)
    const nameEl = document.createElement('div')
    nameEl.className = 'export-msg-name'
    nameEl.style.cssText = 'font-size:12px;font-weight:600;color:#6b7280;line-height:1.4;margin:0 0 8px;'
    if (isUser) {
      nameEl.textContent = userName
    } else if (isSystem) {
      nameEl.textContent = roleLabels.system
    } else {
      nameEl.textContent = options?.getModelLabel?.(message) || roleLabels.assistant
    }
    card.appendChild(nameEl)

    const body = document.createElement('div')
    body.className = 'export-md'
    body.innerHTML = renderExportMarkdownHtml(getExportMessageContent(message, options))
    card.appendChild(body)

    // Footer: time only
    if (message.created_at != null) {
      const footer = document.createElement('div')
      footer.style.cssText = 'margin-top:8px;font-size:11px;line-height:1.4;color:#9ca3af;'
      footer.textContent = formatTime(message.created_at)
      card.appendChild(footer)
    }

    host.appendChild(card)
  }

  document.body.appendChild(host)
  try {
    const { default: html2canvas } = await import('html2canvas')
    const canvas = await html2canvas(host, {
      useCORS: true,
      scale: 2,
      backgroundColor: '#ffffff',
      width: host.scrollWidth,
      height: host.scrollHeight,
      windowWidth: host.scrollWidth,
      windowHeight: host.scrollHeight,
    })
    return canvasToPngFile(canvas, title)
  } finally {
    host.remove()
  }
}

export function buildJsonTranscript(messages: Message[], title: string, options?: TranscriptExportOptions) {
  const data = {
    title,
    exported_at: new Date().toISOString(),
    messages: messages.map((m) => {
      const time = resolveExportMessageTime(m, options)
      return {
        role: m.role,
        // Display name: user profile / model name / system label (same as PNG/md/txt)
        name: resolveExportSpeakerLabel(m, options),
        content: getExportMessageContent(m, options),
        ...(options?.includeThinking === false ? {} : { thinking: m.thinking }),
        ...(time ? { time } : {}),
        created_at: m.created_at,
      }
    }),
  }
  return JSON.stringify(data, null, 2)
}

export async function copyTranscript(
  messages: Message[],
  title: string,
  format: 'markdown' | 'text',
  options?: TranscriptExportOptions,
) {
  const content = format === 'markdown'
    ? buildMarkdownTranscript(messages, title, options)
    : buildTextTranscript(messages, title, options)
  await writeToClipboard(content)
  return true
}

export async function exportAsMarkdown(messages: Message[], title: string, options?: TranscriptExportOptions) {
  return saveFile(sanitizeExportFilename(title, 'md', 'chat'), buildMarkdownTranscript(messages, title, options), [{ name: 'Markdown', extensions: ['md'] }])
}

export async function exportAsText(messages: Message[], title: string, options?: TranscriptExportOptions) {
  return saveFile(sanitizeExportFilename(title, 'txt', 'chat'), buildTextTranscript(messages, title, options), [{ name: 'Text', extensions: ['txt'] }])
}

export async function exportAsJSON(messages: Message[], title: string, options?: TranscriptExportOptions) {
  return saveFile(sanitizeExportFilename(title, 'json', 'chat'), buildJsonTranscript(messages, title, options), [{ name: 'JSON', extensions: ['json'] }])
}
