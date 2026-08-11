import type { Conversation, ConversationCategory } from '@/types'
import {
  compareConversationOrder,
  getUncategorizedConversationGroup,
  UNCATEGORIZED_GROUP_ORDER,
} from '@/lib/conversationOrder'

export {
  compareConversationOrder,
  getUncategorizedConversationGroup,
  UNCATEGORIZED_GROUP_ORDER,
} from '@/lib/conversationOrder'

export const NATIVE_LIST_MAX_ROWS = 159
export const VIRTUAL_LIST_MIN_ROWS = 160
export const SIDEBAR_OVERSCAN = 8

export type ConversationListRow =
  | {
    type: 'groupHeader'
    key: string
    group: string
    category: ConversationCategory | null
    collapsible: boolean
    expanded: boolean
  }
  | {
    type: 'conversation'
    key: string
    group: string
    conversation: Conversation
    isChild: boolean
    childCount: number
    expanded: boolean
  }
  | {
    type: 'emptyCategory'
    key: string
    group: string
    category: ConversationCategory
  }

interface BuildConversationRowsInput {
  conversations: readonly Conversation[]
  categories: readonly ConversationCategory[]
  expandedParentIds: ReadonlySet<string>
  expandedGroupKeys: ReadonlySet<string>
  nowSeconds?: number
}

export interface ConversationReorderPlan {
  categoryId: string | null
  conversationIds: string[]
}

/**
 * Build the complete final order expected by the atomic reorder command.
 * Only top-level conversations are draggable, and a move may not cross the
 * visible category/date/pin group boundary.
 */
export function planConversationReorder(
  rows: readonly ConversationListRow[],
  activeConversationId: string,
  overConversationId: string,
): ConversationReorderPlan | null {
  if (activeConversationId === overConversationId) return null

  const activeRow = rows.find((row) => (
    row.type === 'conversation' && row.conversation.id === activeConversationId
  ))
  const overRow = rows.find((row) => (
    row.type === 'conversation' && row.conversation.id === overConversationId
  ))
  if (
    activeRow?.type !== 'conversation'
    || overRow?.type !== 'conversation'
    || activeRow.isChild
    || overRow.isChild
    || activeRow.group !== overRow.group
  ) return null

  const categoryId = activeRow.conversation.category_id ?? null
  const containerRows = rows.filter((row): row is Extract<ConversationListRow, { type: 'conversation' }> => (
    row.type === 'conversation'
    && !row.isChild
    && (row.conversation.category_id ?? null) === categoryId
  ))
  const conversationIds = containerRows.map((row) => row.conversation.id)
  const oldIndex = conversationIds.indexOf(activeConversationId)
  const newIndex = conversationIds.indexOf(overConversationId)
  if (oldIndex === -1 || newIndex === -1) return null

  const reorderedIds = [...conversationIds]
  reorderedIds.splice(oldIndex, 1)
  reorderedIds.splice(newIndex, 0, activeConversationId)
  return { categoryId, conversationIds: reorderedIds }
}

export function filterConversationsWithParents(
  conversations: readonly Conversation[],
  rawQuery: string,
): readonly Conversation[] {
  const query = rawQuery.trim().toLocaleLowerCase()
  if (!query) return conversations

  const conversationById = new Map(conversations.map((item) => [item.id, item]))
  const includedIds = new Set<string>()
  for (const conversation of conversations) {
    if (!conversation.title.toLocaleLowerCase().includes(query)) continue
    includedIds.add(conversation.id)

    let parentId = conversation.parent_conversation_id
    while (parentId && !includedIds.has(parentId)) {
      includedIds.add(parentId)
      parentId = conversationById.get(parentId)?.parent_conversation_id ?? null
    }
  }

  return conversations.filter((conversation) => includedIds.has(conversation.id))
}

export function getSearchExpandedParentIds(
  conversations: readonly Conversation[],
  rawQuery: string,
): ReadonlySet<string> {
  const query = rawQuery.trim().toLocaleLowerCase()
  const expandedParentIds = new Set<string>()
  if (!query) return expandedParentIds

  const conversationById = new Map(conversations.map((item) => [item.id, item]))
  for (const conversation of conversations) {
    if (!conversation.title.toLocaleLowerCase().includes(query)) continue

    let parentId = conversation.parent_conversation_id
    while (parentId && !expandedParentIds.has(parentId)) {
      expandedParentIds.add(parentId)
      parentId = conversationById.get(parentId)?.parent_conversation_id ?? null
    }
  }

  return expandedParentIds
}

export function buildConversationRows({
  conversations,
  categories,
  expandedParentIds,
  expandedGroupKeys,
  nowSeconds = Date.now() / 1000,
}: BuildConversationRowsInput): ConversationListRow[] {
  const childrenByParent = new Map<string, Conversation[]>()
  const topLevel: Conversation[] = []

  for (const conversation of conversations) {
    if (conversation.parent_conversation_id) {
      const children = childrenByParent.get(conversation.parent_conversation_id)
      if (children) children.push(conversation)
      else childrenByParent.set(conversation.parent_conversation_id, [conversation])
    } else {
      topLevel.push(conversation)
    }
  }

  for (const children of childrenByParent.values()) {
    children.sort(compareConversationOrder)
  }

  const conversationsByCategory = new Map<string, Conversation[]>()
  const uncategorized: Conversation[] = []
  for (const conversation of topLevel) {
    if (conversation.category_id) {
      const grouped = conversationsByCategory.get(conversation.category_id)
      if (grouped) grouped.push(conversation)
      else conversationsByCategory.set(conversation.category_id, [conversation])
    } else {
      uncategorized.push(conversation)
    }
  }

  const rows: ConversationListRow[] = []
  const pushConversation = (conversation: Conversation, group: string, isChild = false) => {
    const children = childrenByParent.get(conversation.id) ?? []
    rows.push({
      type: 'conversation',
      key: `conversation:${conversation.id}`,
      group,
      conversation,
      isChild,
      childCount: children.length,
      expanded: expandedParentIds.has(conversation.id),
    })
    if (!expandedParentIds.has(conversation.id)) return
    for (const child of children) {
      rows.push({
        type: 'conversation',
        key: `conversation:${child.id}`,
        group,
        conversation: child,
        isChild: true,
        childCount: 0,
        expanded: false,
      })
    }
  }

  for (const category of categories) {
    const group = `cat:${category.id}`
    const expanded = expandedGroupKeys.has(group)
    rows.push({
      type: 'groupHeader',
      key: `group:${group}`,
      group,
      category,
      collapsible: true,
      expanded,
    })
    if (!expanded) continue

    const grouped = conversationsByCategory.get(category.id)
    if (grouped?.length) {
      for (const conversation of [...grouped].sort(compareConversationOrder)) {
        pushConversation(conversation, group)
      }
    } else {
      rows.push({
        type: 'emptyCategory',
        key: `empty:${category.id}`,
        group,
        category,
      })
    }
  }

  const uncategorizedGroups = new Map<string, Conversation[]>()
  for (const conversation of uncategorized) {
    const group = getUncategorizedConversationGroup(conversation, nowSeconds)
    const grouped = uncategorizedGroups.get(group)
    if (grouped) grouped.push(conversation)
    else uncategorizedGroups.set(group, [conversation])
  }

  for (const group of UNCATEGORIZED_GROUP_ORDER) {
    const grouped = uncategorizedGroups.get(group)
    if (!grouped?.length) continue
    rows.push({
      type: 'groupHeader',
      key: `group:${group}`,
      group,
      category: null,
      collapsible: false,
      expanded: true,
    })
    for (const conversation of [...grouped].sort(compareConversationOrder)) {
      pushConversation(conversation, group)
    }
  }

  return rows
}
