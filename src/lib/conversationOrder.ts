import type { Conversation } from '@/types'

export const UNCATEGORIZED_GROUP_ORDER = [
  'pinned',
  'today',
  'yesterday',
  'thisWeek',
  'thisMonth',
  'earlier',
] as const

export type UncategorizedConversationGroup = typeof UNCATEGORIZED_GROUP_ORDER[number]

export function compareConversationOrder(a: Conversation, b: Conversation): number {
  const sortOrderDiff = a.sort_order - b.sort_order
  if (sortOrderDiff !== 0) return sortOrderDiff
  const updatedAtDiff = b.updated_at - a.updated_at
  if (updatedAtDiff !== 0) return updatedAtDiff
  return a.id.localeCompare(b.id)
}

function getDateGroup(timestamp: number, nowSeconds: number): UncategorizedConversationGroup {
  const now = new Date(nowSeconds * 1000)
  const date = new Date(timestamp * 1000)
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const startOfYesterday = new Date(startOfToday.getTime() - 86_400_000)
  const startOfWeek = new Date(startOfToday.getTime() - startOfToday.getDay() * 86_400_000)
  const startOfMonth = new Date(now.getFullYear(), now.getMonth(), 1)

  if (date >= startOfToday) return 'today'
  if (date >= startOfYesterday) return 'yesterday'
  if (date >= startOfWeek) return 'thisWeek'
  if (date >= startOfMonth) return 'thisMonth'
  return 'earlier'
}

export function getUncategorizedConversationGroup(
  conversation: Conversation,
  nowSeconds: number,
): UncategorizedConversationGroup {
  return conversation.is_pinned
    ? 'pinned'
    : getDateGroup(conversation.updated_at, nowSeconds)
}
