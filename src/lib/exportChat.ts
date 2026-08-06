import { isTauri } from '@/lib/invoke'
import { stripAqbotTags } from '@/lib/chatMarkdown'
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

export interface TranscriptExportOptions {
  includeThinking?: boolean;
}

function getExportMessageContent(message: Message, options?: TranscriptExportOptions) {
  if (options?.includeThinking === false) {
    return stripAqbotTags(message.content, { stripThink: message.role !== 'user' })
  }
  return message.content
}

export function buildMarkdownTranscript(messages: Message[], title: string, options?: TranscriptExportOptions) {
  const lines: string[] = [`# ${title}`, '']
  for (const m of messages) {
    const role = m.role === 'user' ? 'User' : m.role === 'system' ? 'System' : 'Assistant'
    lines.push(`## ${role}`, '', getExportMessageContent(m, options), '', '---', '')
  }
  return lines.join('\n')
}

export function buildTextTranscript(messages: Message[], title: string, options?: TranscriptExportOptions) {
  const lines: string[] = [title, '='.repeat(title.length), '']
  for (const m of messages) {
    const role = m.role === 'user' ? 'User' : m.role === 'system' ? 'System' : 'Assistant'
    lines.push(`[${role}]`, '', getExportMessageContent(m, options), '', '---', '')
  }
  return lines.join('\n')
}

async function canvasToPngFile(canvas: HTMLCanvasElement, title: string) {
  if (isTauri()) {
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
    if (!blob) return false
    const buffer = new Uint8Array(await blob.arrayBuffer())
    return saveFile(`${title}.png`, buffer, [{ name: 'PNG Image', extensions: ['png'] }])
  }

  // Browser fallback
  const link = document.createElement('a')
  link.download = `${title}.png`
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

/**
 * Render selected messages into a clean off-screen card layout, then capture PNG.
 * Avoids viewport clipping and action-icon layout bugs from the live chat DOM.
 */
export async function exportMessagesAsPNG(
  messages: Message[],
  title: string,
  options?: TranscriptExportOptions,
) {
  if (messages.length === 0) return false

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

  const heading = document.createElement('div')
  heading.style.cssText = 'font-size:18px;font-weight:600;margin:0 0 4px;line-height:1.4;'
  heading.textContent = title
  host.appendChild(heading)

  const meta = document.createElement('div')
  meta.style.cssText = 'font-size:12px;color:#6b7280;margin:0 0 20px;'
  meta.textContent = new Date().toLocaleString()
  host.appendChild(meta)

  for (const message of messages) {
    const card = document.createElement('div')
    const isUser = message.role === 'user'
    card.style.cssText = [
      'margin:0 0 14px',
      'padding:12px 14px',
      'border-radius:12px',
      `background:${isUser ? '#eff6ff' : '#f9fafb'}`,
      `border:1px solid ${isUser ? '#dbeafe' : '#e5e7eb'}`,
    ].join(';')

    const role = document.createElement('div')
    role.style.cssText = 'font-size:12px;font-weight:600;color:#6b7280;margin:0 0 8px;'
    role.textContent = isUser ? 'User' : message.role === 'system' ? 'System' : 'Assistant'
    card.appendChild(role)

    const body = document.createElement('div')
    body.style.cssText = 'font-size:14px;line-height:1.65;white-space:pre-wrap;word-break:break-word;'
    body.textContent = getExportMessageContent(message, options)
    card.appendChild(body)

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
    messages: messages.map((m) => ({
      role: m.role,
      content: getExportMessageContent(m, options),
      ...(options?.includeThinking === false ? {} : { thinking: m.thinking }),
      created_at: m.created_at,
    })),
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
  return saveFile(`${title}.md`, buildMarkdownTranscript(messages, title, options), [{ name: 'Markdown', extensions: ['md'] }])
}

export async function exportAsText(messages: Message[], title: string, options?: TranscriptExportOptions) {
  return saveFile(`${title}.txt`, buildTextTranscript(messages, title, options), [{ name: 'Text', extensions: ['txt'] }])
}

export async function exportAsJSON(messages: Message[], title: string, options?: TranscriptExportOptions) {
  return saveFile(`${title}.json`, buildJsonTranscript(messages, title, options), [{ name: 'JSON', extensions: ['json'] }])
}
