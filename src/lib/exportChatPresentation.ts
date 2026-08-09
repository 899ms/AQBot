import i18n from '@/i18n'
import { formatChatTime } from '@/components/chat/chatTime'
import type { Message, ProviderConfig } from '@/types'
import type { ExportMessagesPngOptions, ExportPngTheme, TranscriptExportOptions } from './exportChat'

export type ExportPresentationInput = {
  /** Display name for user messages; empty falls back to i18n "you" */
  userName?: string | null
  theme: ExportPngTheme
  providers: ProviderConfig[]
  /** Conversation-level fallback when message.model_id is empty */
  conversationModelId?: string | null
  conversationProviderId?: string | null
}

function resolveModelLabel(
  message: Message,
  providers: ProviderConfig[],
  conversationModelId?: string | null,
  conversationProviderId?: string | null,
): string | undefined {
  const mid = message.model_id ?? conversationModelId
  const pid = message.provider_id ?? conversationProviderId
  if (!mid) return undefined
  const provider = pid ? providers.find((p) => p.id === pid) : undefined
  const model = provider?.models.find((m) => m.model_id === mid)
  // Title uses model name only (no provider/platform prefix).
  if (model?.name) return model.name
  for (const p of providers) {
    const m = p.models.find((item) => item.model_id === mid)
    if (m?.name) return m.name
  }
  return mid
}

/** Shared presentation options for PNG / Markdown / Text / copy (i18n, model name, time). */
export function buildExportOptions(input: ExportPresentationInput): ExportMessagesPngOptions {
  const { userName, theme, providers, conversationModelId, conversationProviderId } = input

  return {
    roleLabels: {
      user: i18n.t('chat.you'),
      assistant: i18n.t('chat.assistant'),
      system: i18n.t('chat.system'),
    },
    userName: userName?.trim() || undefined,
    theme,
    getModelLabel: (message) => resolveModelLabel(
      message,
      providers,
      conversationModelId,
      conversationProviderId,
    ),
    formatTime: (createdAt) => formatChatTime(createdAt),
  }
}

/** @deprecated Prefer buildExportOptions — same implementation. */
export const buildExportPngOptions = buildExportOptions

/** Transcript-only subset (no theme) for callers that only need md/text. */
export function buildTranscriptExportOptions(
  input: Omit<ExportPresentationInput, 'theme'> & { theme?: ExportPngTheme },
): TranscriptExportOptions {
  const full = buildExportOptions({
    ...input,
    theme: input.theme ?? {
      colorPrimary: '#1677ff',
      colorPrimaryBg: '#e6f4ff',
      colorPrimaryBorder: '#91caff',
    },
  })
  return {
    roleLabels: full.roleLabels,
    userName: full.userName,
    getModelLabel: full.getModelLabel,
    formatTime: full.formatTime,
  }
}
