import type { NodeComponentProps } from 'markstream-react';
import { getCustomAttr, type CustomNodeAttrs } from '@/components/chat/chatMarkdownShared';
import { useAcpStore, type AcpPlanDocument } from '@/stores/acpStore';
import { AcpPlanDocumentCard, requestAcpPlanAddToContext } from './AcpPlanDocumentCard';

function decodeXmlTextEntities(value: string): string {
  return value
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');
}

function normalizeStatus(value: string | undefined): AcpPlanDocument['status'] {
  const id = String(value ?? '').toLowerCase().replace(/[^a-z0-9]/g, '');
  if (id === 'approved' || id === 'approve') return 'approved';
  if (id === 'cancelled' || id === 'cancel') return 'cancelled';
  if (id === 'abandoned' || id === 'abandon') return 'abandoned';
  if (id === 'expired') return 'expired';
  if (id === 'pending') return 'pending';
  return 'approved';
}

/**
 * Inline plan-review card for ACP messages — placed chronologically via
 * `<acp-plan>` markers injected when plan mode exits for review.
 *
 * Prefer the live store document; after a page reload fall back to the
 * marker body/attrs so the card still renders from persisted message text.
 */
export function AcpPlanNode(props: NodeComponentProps<{
  type: 'acp-plan';
  content: string;
  attrs?: CustomNodeAttrs;
}>) {
  const { node } = props;
  const planId = getCustomAttr(node.attrs, 'id') ?? '';
  const messageId = getCustomAttr(node.attrs, 'message') ?? '';
  const statusAttr = getCustomAttr(node.attrs, 'status');
  const titleAttr = getCustomAttr(node.attrs, 'title');
  const activeThreadId = useAcpStore((state) => state.activeThreadId ?? '');
  const storeDocument = useAcpStore((state) => {
    const threadId = state.activeThreadId ?? '';
    const docs = state.planDocumentsByThread[threadId] ?? [];
    const byId = docs.find((item) => item.id === planId);
    if (byId) return byId;
    if (!messageId) return undefined;
    return docs.find((item) => item.messageId === messageId && item.id === planId);
  });

  const markerContent = decodeXmlTextEntities(String(node.content ?? '')).trim();
  const document: AcpPlanDocument | undefined = storeDocument ?? (
    planId && (markerContent || titleAttr)
      ? {
          id: planId,
          threadId: activeThreadId,
          messageId: messageId || undefined,
          content: markerContent || titleAttr || '',
          title: titleAttr || undefined,
          status: normalizeStatus(statusAttr),
          sequence: 0,
          createdAt: new Date(0).toISOString(),
        }
      : undefined
  );

  // Pending reviews are handled in the composer to avoid duplicate bodies.
  if (!document || document.status === 'pending') return null;

  // Prefer longer body so a rich store doc wins over a short title marker,
  // while a full marker body can still recover after a cold reload.
  const resolved: AcpPlanDocument = {
    ...document,
    content: (document.content?.length ?? 0) >= markerContent.length
      ? document.content
      : (markerContent || document.content),
  };

  return (
    <div
      className="acp-plan-node"
      data-type="acp-plan"
      data-plan-id={planId}
      style={{
        width: '100%',
        maxWidth: '100%',
        minWidth: 0,
        margin: '8px 0',
        display: 'block',
        boxSizing: 'border-box',
      }}
    >
      <AcpPlanDocumentCard
        document={resolved}
        onAddToContext={requestAcpPlanAddToContext}
      />
    </div>
  );
}
