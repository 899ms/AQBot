import { useState } from 'react';
import { SyncOutlined } from '@ant-design/icons';
import { Typography, theme } from 'antd';
import {
  ChevronDown,
  Code,
  FileCode,
  FileText,
  FileType,
  Zap,
} from 'lucide-react';
import type { NodeComponentProps } from 'markstream-react';
import { useTranslation } from 'react-i18next';
import { getCustomAttr, type CustomNodeAttrs } from '@/components/chat/chatMarkdownShared';
import { useAcpStore } from '@/stores/acpStore';

const toolCallIcons: Record<string, React.ReactNode> = {
  bash: <Code size={14} />,
  shell: <Code size={14} />,
  terminal: <Code size={14} />,
  execute: <Code size={14} />,
  write: <FileCode size={14} />,
  read: <FileText size={14} />,
  edit: <FileCode size={14} />,
  glob: <FileType size={14} />,
  grep: <FileText size={14} />,
  ls: <FileType size={14} />,
  search: <FileText size={14} />,
};

function getInlineToolIcon(toolName: string): React.ReactNode {
  const lower = toolName.toLowerCase();
  for (const [key, icon] of Object.entries(toolCallIcons)) {
    if (lower.includes(key)) return icon;
  }
  return <Zap size={14} />;
}

const toolCallStatusColors: Record<string, string> = {
  queued: '#faad14',
  running: '#1890ff',
  success: '#52c41a',
  error: '#ff4d4f',
  cancelled: '#8c8c8c',
};

/**
 * Inline tool-call chip for ACP messages — mirrors ChatView `ToolCallNode`
 * but reads live status/input/output from `acpStore`.
 */
export function AcpToolCallNode(props: NodeComponentProps<{
  type: 'tool-call';
  content: string;
  attrs?: CustomNodeAttrs;
}>) {
  const { node } = props;
  const { token } = theme.useToken();
  const { t } = useTranslation();
  const execId = getCustomAttr(node.attrs, 'id') ?? '';
  const tc = useAcpStore((s) => s.toolCalls[execId]);
  const [expanded, setExpanded] = useState(false);

  const toolName = getCustomAttr(node.attrs, 'name') ?? tc?.toolName ?? 'tool';
  const summary = String(node.content ?? '');

  // History reload has markers but no live state → treat as success
  const status = tc?.status ?? 'success';
  const statusColor = toolCallStatusColors[status] || token.colorTextSecondary;
  const isLoading = status === 'queued' || status === 'running';
  const hasDetails = !!(tc && (tc.input || tc.output));

  return (
    <div style={{ margin: '4px 0' }}>
      <div
        onClick={() => hasDetails && setExpanded(!expanded)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '4px 10px',
          borderRadius: token.borderRadius,
          backgroundColor: token.colorFillQuaternary,
          border: `1px solid ${token.colorBorderSecondary}`,
          fontSize: 13,
          lineHeight: '20px',
          fontFamily: 'monospace',
          cursor: hasDetails ? 'pointer' : 'default',
          userSelect: 'none',
        }}
      >
        <span style={{ color: statusColor, display: 'flex', alignItems: 'center', flexShrink: 0 }}>
          {getInlineToolIcon(toolName)}
        </span>
        <span style={{ fontWeight: 500, flexShrink: 0 }}>{toolName}</span>
        {summary && (
          <>
            <span style={{ color: token.colorTextQuaternary }}>›</span>
            <Typography.Text
              type="secondary"
              ellipsis
              style={{ fontSize: 12, flex: 1, minWidth: 0 }}
            >
              {summary}
            </Typography.Text>
          </>
        )}
        {isLoading ? (
          <SyncOutlined style={{ fontSize: 12, color: statusColor }} spin />
        ) : (
          <span
            style={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              backgroundColor: statusColor,
              flexShrink: 0,
            }}
          />
        )}
        {hasDetails && (
          <span
            style={{
              color: token.colorTextSecondary,
              display: 'flex',
              alignItems: 'center',
              flexShrink: 0,
              transition: 'transform 0.2s',
              transform: expanded ? 'rotate(180deg)' : 'rotate(0deg)',
            }}
          >
            <ChevronDown size={14} />
          </span>
        )}
      </div>
      {expanded && hasDetails && tc && (
        <div
          style={{
            margin: '2px 0 0',
            padding: '6px 10px',
            borderRadius: token.borderRadius,
            backgroundColor: token.colorFillQuaternary,
            border: `1px solid ${token.colorBorderSecondary}`,
            borderTop: 'none',
            fontSize: 12,
            display: 'flex',
            flexDirection: 'column',
            gap: 4,
          }}
        >
          {tc.input && (
            <details style={{ margin: 0 }}>
              <summary
                style={{
                  fontSize: 12,
                  color: token.colorTextSecondary,
                  cursor: 'pointer',
                  userSelect: 'none',
                }}
              >
                {t('chat.inspector.toolInput', '输入参数')}
              </summary>
              <pre
                style={{
                  margin: '4px 0 0',
                  padding: 8,
                  fontSize: 11,
                  fontFamily: 'monospace',
                  backgroundColor: token.colorBgTextHover,
                  borderRadius: token.borderRadius,
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-all',
                  maxHeight: 200,
                  overflow: 'auto',
                }}
              >
                {tc.input}
              </pre>
            </details>
          )}
          {tc.output && (
            <details style={{ margin: 0 }}>
              <summary
                style={{
                  fontSize: 12,
                  color: token.colorTextSecondary,
                  cursor: 'pointer',
                  userSelect: 'none',
                }}
              >
                {t('chat.inspector.toolOutput', '执行结果')}
              </summary>
              <pre
                className="aqbot-chat-tool-output-pre"
                style={{
                  margin: '4px 0 0',
                  padding: 8,
                  fontSize: 11,
                  fontFamily: 'monospace',
                  backgroundColor: token.colorBgTextHover,
                  borderRadius: token.borderRadius,
                  whiteSpace: 'pre',
                  overflow: 'auto',
                  maxHeight: 200,
                  color: tc.status === 'error' ? token.colorError : undefined,
                }}
              >
                {tc.output}
              </pre>
            </details>
          )}
        </div>
      )}
    </div>
  );
}
