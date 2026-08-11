import type { AcpMessage } from '@/types/acp';

export interface AcpPlanEntry {
  content: string;
  status: string;
  priority?: string;
}

export interface AcpPlanState {
  entries: AcpPlanEntry[];
  completed: number;
  total: number;
}

/** Persisted plan-review document shown in the conversation timeline after exit. */
export interface AcpPlanDocument {
  id: string;
  threadId: string;
  messageId?: string;
  content: string;
  title?: string;
  status: 'pending' | 'approved' | 'cancelled' | 'abandoned' | 'expired';
  sequence: number;
  createdAt: string;
  feedback?: string;
}

export function extractPlanDocumentContent(
  input: Record<string, unknown> | null | undefined,
  extras?: { description?: string | null; title?: string | null },
): string {
  const source = input ?? {};
  const candidates = [
    extras?.description,
    source.planContent,
    source.plan_content,
    source.plan,
    source.content,
    source.description,
    extras?.title,
    source.title,
  ];
  for (const value of candidates) {
    if (typeof value === 'string' && value.trim()) return value;
  }
  return '';
}

export function planDocumentStatusFromResolution(
  optionId: string | undefined,
  reason: 'selected' | 'cancelled' | 'expired' | undefined,
  optionKind?: string,
): AcpPlanDocument['status'] {
  if (reason === 'expired') return 'expired';
  const id = String(optionId ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
  const kind = String(optionKind ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
  if (id === 'abandoned' || id === 'abandon') return 'abandoned';
  if (reason === 'cancelled') return 'abandoned';
  if (kind.includes('reject') || kind.includes('deny')) return 'cancelled';
  if (kind.includes('allow')) return 'approved';
  if (id === 'approved' || id === 'approve' || id === 'implementplan') return 'approved';
  if (
    id === 'cancelled'
    || id === 'cancel'
    || id === 'reviseplan'
    || id === 'plan'
  ) return 'cancelled';
  if (reason === 'selected') return 'approved';
  return 'expired';
}

export function upsertPlanDocument(
  byThread: Record<string, AcpPlanDocument[]>,
  document: AcpPlanDocument,
): Record<string, AcpPlanDocument[]> {
  const existing = byThread[document.threadId] ?? [];
  const index = existing.findIndex((item) => item.id === document.id);
  const nextList = index >= 0
    ? existing.map((item, i) => (i === index
      ? {
          ...item,
          ...document,
          // Keep the earliest sequence/createdAt so timeline order is stable.
          sequence: item.sequence,
          createdAt: item.createdAt,
          content: document.content || item.content,
          title: document.title ?? item.title,
          messageId: document.messageId ?? item.messageId,
          feedback: document.feedback ?? item.feedback,
        }
      : item))
    : [...existing, document];
  return { ...byThread, [document.threadId]: nextList };
}

export function resolvePlanDocument(
  byThread: Record<string, AcpPlanDocument[]>,
  requestId: string,
  patch: Partial<Pick<AcpPlanDocument, 'status' | 'feedback' | 'messageId' | 'content' | 'title'>>,
): Record<string, AcpPlanDocument[]> {
  let changed = false;
  const next: Record<string, AcpPlanDocument[]> = {};
  for (const [threadId, docs] of Object.entries(byThread)) {
    next[threadId] = docs.map((doc) => {
      if (doc.id !== requestId) return doc;
      changed = true;
      return {
        ...doc,
        ...patch,
        content: patch.content || doc.content,
        title: patch.title ?? doc.title,
        messageId: patch.messageId ?? doc.messageId,
        feedback: patch.feedback ?? doc.feedback,
      };
    });
  }
  return changed ? next : byThread;
}

/** When a turn ends, keep plan bodies but mark still-pending reviews as expired. */
export function finalizePendingPlanDocuments(
  byThread: Record<string, AcpPlanDocument[]>,
  threadId: string,
): Record<string, AcpPlanDocument[]> {
  const docs = byThread[threadId];
  if (!docs?.some((doc) => doc.status === 'pending')) return byThread;
  return {
    ...byThread,
    [threadId]: docs.map((doc) => (
      doc.status === 'pending' ? { ...doc, status: 'expired' as const } : doc
    )),
  };
}

/**
 * Session plan progress (todo checklist). Only structured ACP plan entries
 * count — never markdown `planContent` from plan-review documents, which used
 * to produce garbage todos like form field labels.
 *
 * Returns `null` when the payload is not a real progress update so callers can
 * leave the existing checklist alone.
 */
export function normalizePlan(raw: Record<string, unknown>): AcpPlanState | null {
  const kind = String(raw.kind ?? raw.sessionUpdate ?? raw.session_update ?? '')
    .toLowerCase()
    .replace(/[^a-z0-9_]/g, '');
  // Plan-review documents and exit-plan-mode payloads are not progress lists.
  if (kind === 'plan_review' || kind === 'planreview' || kind.includes('planreview')) {
    return null;
  }
  // Document-only payloads (planContent without structured entries).
  const hasStructuredEntries = Array.isArray(raw.entries)
    || Array.isArray((raw.plan as Record<string, unknown> | undefined)?.entries);
  if (!hasStructuredEntries) {
    return null;
  }

  const source = Array.isArray(raw.entries)
    ? raw.entries
    : Array.isArray((raw.plan as Record<string, unknown> | undefined)?.entries)
      ? ((raw.plan as Record<string, unknown>).entries as unknown[])
      : [];

  const entries = source
    .map((item) => {
      const entry = (item ?? {}) as Record<string, unknown>;
      const content = String(entry.content ?? entry.title ?? entry.description ?? '').trim();
      if (!content) return null;
      return {
        content,
        status: String(entry.status ?? 'pending').toLowerCase(),
        ...(entry.priority ? { priority: String(entry.priority) } : {}),
      };
    })
    .filter((entry): entry is AcpPlanEntry => entry != null);

  // Empty structured array is a valid "clear progress" signal from the agent.
  const completed = entries.filter((entry) =>
    ['completed', 'complete', 'done'].includes(entry.status),
  ).length;
  return { entries, completed, total: entries.length };
}

function decodeXmlTextEntities(value: string): string {
  return value
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');
}

function parseHtmlAttrMap(openTag: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  const re = /([:@A-Za-z_][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(openTag)) != null) {
    const key = match[1].toLowerCase();
    const value = match[2] ?? match[3] ?? match[4] ?? '';
    attrs[key] = decodeXmlTextEntities(value);
  }
  return attrs;
}

function normalizePlanDocumentStatus(value: string | undefined): AcpPlanDocument['status'] {
  const id = String(value ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
  if (id === 'approved' || id === 'approve') return 'approved';
  if (id === 'cancelled' || id === 'cancel') return 'cancelled';
  if (id === 'abandoned' || id === 'abandon') return 'abandoned';
  if (id === 'expired') return 'expired';
  if (id === 'pending') return 'pending';
  // Markers that survived a completed turn without a status default to approved
  // so the card remains readable after reload.
  return 'approved';
}

/**
 * Rebuild plan-review documents from inline `<acp-plan>` markers in message
 * content (the durable source of truth after a refresh).
 */
export function persistedPlanDocuments(messages: AcpMessage[]): Record<string, AcpPlanDocument[]> {
  const byThread: Record<string, AcpPlanDocument[]> = {};
  const re = /<acp-plan\b([^>]*)>([\s\S]*?)<\/acp-plan>/gi;
  for (const message of messages) {
    if (message.role !== 'assistant' || !message.content) continue;
    let match: RegExpExecArray | null;
    re.lastIndex = 0;
    let sequence = 0;
    while ((match = re.exec(message.content)) != null) {
      const attrs = parseHtmlAttrMap(match[1] ?? '');
      if (attrs['data-aqbot'] !== '1' && attrs['data-aqbot'] !== 'true') {
        // Still accept markers we emit (always have data-aqbot="1"), but be lenient.
      }
      const id = attrs.id?.trim();
      if (!id) continue;
      const body = decodeXmlTextEntities(match[2] ?? '').trim();
      const title = attrs.title?.trim() || undefined;
      const content = body || title || '';
      if (!content) continue;
      const document: AcpPlanDocument = {
        id,
        threadId: message.thread_id,
        messageId: attrs.message?.trim() || message.id,
        content,
        title,
        status: normalizePlanDocumentStatus(attrs.status),
        sequence: sequence++,
        createdAt: message.created_at,
      };
      const list = byThread[document.threadId] ?? [];
      const index = list.findIndex((item) => item.id === document.id);
      if (index >= 0) {
        list[index] = {
          ...list[index],
          ...document,
          // Prefer longer content if a later message revisits the same id.
          content: document.content.length >= list[index].content.length
            ? document.content
            : list[index].content,
        };
      } else {
        list.push(document);
      }
      byThread[document.threadId] = list;
    }
  }
  return byThread;
}
