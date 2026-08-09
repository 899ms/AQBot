import React, { useState, useMemo } from 'react';
import { Card, Input, Button, Space, Typography, Checkbox, theme } from 'antd';
import { MessageCircleQuestion, CheckCircle2 } from 'lucide-react';
import { useAgentStore } from '@/stores';
import { useTranslation } from 'react-i18next';

const { Text } = Typography;
const { TextArea } = Input;

interface AskUserCardProps {
  askId: string;
  conversationId: string;
  question: string;
  options?: string[];
}

const AskUserCard: React.FC<AskUserCardProps> = ({ askId, question, options }) => {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [answer, setAnswer] = useState('');
  const [selectedOptions, setSelectedOptions] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const respondAskUser = useAgentStore((s) => s.respondAskUser);

  const hasOptions = options && options.length > 0;

  const canSubmit = useMemo(() => {
    if (hasOptions) return selectedOptions.length > 0 || answer.trim().length > 0;
    return answer.trim().length > 0;
  }, [hasOptions, selectedOptions, answer]);

  const handleToggleOption = (opt: string) => {
    setSelectedOptions((prev) =>
      prev.includes(opt) ? prev.filter((o) => o !== opt) : [...prev, opt],
    );
  };

  const buildAnswer = (): string => {
    const parts: string[] = [];
    if (selectedOptions.length > 0) {
      parts.push(selectedOptions.join(', '));
    }
    if (answer.trim()) {
      parts.push(answer.trim());
    }
    return parts.join('\n');
  };

  const handleSubmit = async () => {
    const finalAnswer = buildAnswer();
    if (!finalAnswer || submitting || submitted) return;
    setSubmitting(true);
    try {
      await respondAskUser(askId, finalAnswer);
      setSubmitted(true);
    } catch {
      setSubmitting(false);
    }
  };

  const questionLines = question.split('\n');

  return (
    <Card
      size="small"
      style={{
        marginTop: 8,
        borderColor: token.colorPrimary,
        opacity: submitted ? 0.7 : 1,
        transition: 'opacity 0.3s',
      }}
      styles={{ body: { padding: '12px 16px' } }}
    >
      <Space direction="vertical" style={{ width: '100%' }} size={8}>
        <Space size={8} align="start">
          <MessageCircleQuestion
            size={16}
            style={{ color: token.colorPrimary, flexShrink: 0, marginTop: 2 }}
          />
          <Text style={{ whiteSpace: 'pre-wrap' }}>
            {questionLines.map((line, i) => (
              <React.Fragment key={i}>
                {i > 0 && <br />}
                {line}
              </React.Fragment>
            ))}
          </Text>
        </Space>

        {hasOptions && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6, paddingLeft: 24 }}>
            {options!.map((opt) => (
              <Checkbox
                key={opt}
                checked={selectedOptions.includes(opt)}
                disabled={submitting || submitted}
                onChange={() => handleToggleOption(opt)}
                style={{ marginLeft: 0 }}
              >
                {opt}
              </Checkbox>
            ))}
          </div>
        )}

        <TextArea
          value={answer}
          onChange={(e) => setAnswer(e.target.value)}
          placeholder={
            hasOptions
              ? t('agent.askUserSupplementPlaceholder')
              : t('agent.askUserPlaceholder')
          }
          autoSize={{ minRows: 1, maxRows: 4 }}
          disabled={submitting || submitted}
          style={{ borderColor: token.colorPrimary }}
          onPressEnter={(e) => {
            if (!e.shiftKey) {
              e.preventDefault();
              handleSubmit();
            }
          }}
        />
        <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
          {submitted ? (
            <Space size={4} style={{ color: token.colorSuccess }}>
              <CheckCircle2 size={14} />
              <Text style={{ color: token.colorSuccess, fontSize: 13 }}>
                {t('agent.askUserSubmitted')}
              </Text>
            </Space>
          ) : (
            <Button
              type="primary"
              size="small"
              onClick={handleSubmit}
              loading={submitting}
              disabled={!canSubmit}
            >
              {t('agent.askUserSubmit')}
            </Button>
          )}
        </div>
      </Space>
    </Card>
  );
};

export default AskUserCard;
