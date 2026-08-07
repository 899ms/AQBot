import React, { useState } from 'react';
import { Button, Card, Space, Tag, Typography, theme } from 'antd';
import { Shield, ShieldCheck, ShieldX, ChevronDown, ChevronRight } from 'lucide-react';
import { useAgentStore } from '@/stores';
import { useTranslation } from 'react-i18next';

const { Text } = Typography;

export interface PermissionOptionButton {
  /** Decision / option id sent to onApprove */
  id: string;
  label: string;
  /** primary | default | danger */
  variant?: 'primary' | 'default' | 'danger';
}

interface PermissionCardProps {
  conversationId: string;
  toolUseId: string;
  toolName: string;
  input: Record<string, unknown>;
  status: 'pending' | 'approved' | 'denied' | 'expired';
  /**
   * Optional override for approve action (e.g. ACP workbench).
   * When omitted, uses the chat agentStore `agent_approve` path.
   */
  onApprove?: (decision: string) => Promise<void>;
  /**
   * Optional custom option buttons. Defaults to Allow Once / Always Allow / Deny
   * (chat agent mode).
   */
  options?: PermissionOptionButton[];
}

const PermissionCard: React.FC<PermissionCardProps> = ({
  conversationId,
  toolUseId,
  toolName,
  input,
  status,
  onApprove,
  options,
}) => {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [expanded, setExpanded] = useState(false);
  const approveToolUse = useAgentStore((state) => state.approveToolUse);
  const [loading, setLoading] = useState<string | null>(null);

  const handleApprove = async (decision: string) => {
    setLoading(decision);
    try {
      if (onApprove) {
        await onApprove(decision);
      } else {
        await approveToolUse(conversationId, toolUseId, decision);
      }
    } catch (e) {
      console.error('[PermissionCard] handleApprove failed:', e);
    } finally {
      setLoading(null);
    }
  };

  const actionOptions: PermissionOptionButton[] = options ?? [
    { id: 'allow_once', label: t('common.allowOnce', 'Allow Once'), variant: 'primary' },
    { id: 'allow_always', label: t('common.allowAlways', 'Always Allow'), variant: 'default' },
    { id: 'deny', label: t('common.deny', 'Deny'), variant: 'danger' },
  ];

  const inputStr = JSON.stringify(input, null, 2);

  const borderColor =
    status === 'pending'
      ? token.colorWarningBorder
      : status === 'approved'
        ? token.colorSuccessBorder
        : status === 'denied'
          ? token.colorErrorBorder
          : token.colorBorderSecondary;

  return (
    <Card
      size="small"
      style={{
        margin: '8px 0',
        borderColor,
        borderRadius: 8,
      }}
    >
      <Space direction="vertical" style={{ width: '100%' }} size={8}>
        {/* Header */}
        <Space align="center">
          <Shield size={16} />
          <Text strong>{t('common.permissionRequired', 'Permission Required')}</Text>
          <Tag>{toolName}</Tag>
        </Space>

        {/* Input preview */}
        <div
          onClick={() => setExpanded(!expanded)}
          style={{ cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4 }}
        >
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t('common.toolInput', 'Tool Input')}
          </Text>
        </div>
        {expanded && (
          <pre
            style={{
              margin: 0,
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
            {inputStr}
          </pre>
        )}

        {/* Action buttons or result */}
        {status === 'pending' ? (
          <Space wrap>
            {actionOptions.map((opt) => (
              <Button
                key={opt.id}
                size="small"
                type={opt.variant === 'primary' ? 'primary' : 'default'}
                danger={opt.variant === 'danger'}
                icon={
                  opt.variant === 'danger'
                    ? <ShieldX size={14} />
                    : <ShieldCheck size={14} />
                }
                loading={loading === opt.id}
                onClick={() => handleApprove(opt.id)}
              >
                {opt.label}
              </Button>
            ))}
          </Space>
        ) : status === 'approved' ? (
          <Space>
            <ShieldCheck size={14} style={{ color: token.colorSuccess }} />
            <Text type="success">{t('common.approved', 'Approved')}</Text>
          </Space>
        ) : status === 'denied' ? (
          <Space>
            <ShieldX size={14} style={{ color: token.colorError }} />
            <Text type="danger">{t('common.denied', 'Denied')}</Text>
          </Space>
        ) : (
          <Space>
            <Text type="warning">⚠️ {t('common.expired', 'Expired (Agent disconnected)')}</Text>
          </Space>
        )}
      </Space>
    </Card>
  );
};

export default PermissionCard;
