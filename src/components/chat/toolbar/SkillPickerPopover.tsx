import { useEffect, useMemo, useState } from 'react';
import { Badge, Button, Checkbox, Empty, Input, Popover, Tooltip, theme } from 'antd';
import { Search, Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useSkillStore } from '@/stores';
import { useUIStore } from '@/stores/uiStore';

export function SkillPickerPopover() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');

  const skills = useSkillStore((s) => s.skills);
  const skillsMeta = useSkillStore((s) => s.skillsMeta);
  const ensureSkillsLoaded = useSkillStore((s) => s.ensureSkillsLoaded);
  const toggleSkill = useSkillStore((s) => s.toggleSkill);
  const setActivePage = useUIStore((s) => s.setActivePage);

  // Load on mount so the badge count is ready the first time Agent mode is shown,
  // not only after the user opens the popover.
  useEffect(() => {
    void ensureSkillsLoaded();
  }, [ensureSkillsLoaded]);

  useEffect(() => {
    if (open) void ensureSkillsLoaded();
  }, [ensureSkillsLoaded, open]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return skills;
    return skills.filter((skill) =>
      skill.name.toLowerCase().includes(q)
      || skill.description.toLowerCase().includes(q)
      || skill.source.toLowerCase().includes(q),
    );
  }, [query, skills]);

  const enabledCount = useMemo(
    () => skills.filter((skill) => skill.enabled).length,
    [skills],
  );
  // Avoid flashing a zero badge while the first load is still in flight.
  const badgeCount = skillsMeta.status === 'ready' || skills.length > 0 ? enabledCount : 0;

  const content = (
    <div style={{ width: 260 }}>
      <Input
        size="small"
        allowClear
        prefix={<Search size={12} />}
        placeholder={t('chat.skill.searchPlaceholder')}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        style={{ marginBottom: 8 }}
      />
      <div style={{ maxHeight: 260, overflowY: 'auto' }}>
        {filtered.length === 0 ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t('chat.skill.empty')}
            style={{ margin: '12px 0' }}
          />
        ) : (
          filtered.map((skill) => (
            <div key={`${skill.sourcePath}-${skill.name}`} style={{ padding: '3px 0' }}>
              <Checkbox
                checked={skill.enabled}
                onChange={(event) => {
                  void toggleSkill(skill.name, event.target.checked);
                }}
              >
                <span style={{ fontSize: 13 }}>{skill.name}</span>
                <span style={{ fontSize: 11, color: token.colorTextSecondary, marginLeft: 6 }}>
                  {skill.source}
                </span>
              </Checkbox>
              {skill.description ? (
                <div
                  style={{
                    fontSize: 11,
                    color: token.colorTextSecondary,
                    marginLeft: 24,
                    marginTop: 2,
                    lineHeight: 1.3,
                    display: '-webkit-box',
                    WebkitLineClamp: 2,
                    WebkitBoxOrient: 'vertical',
                    overflow: 'hidden',
                  }}
                >
                  {skill.description}
                </div>
              ) : null}
            </div>
          ))
        )}
      </div>
      <div style={{ borderTop: `1px solid ${token.colorBorderSecondary}`, marginTop: 8, paddingTop: 8 }}>
        <Button
          type="link"
          size="small"
          style={{ padding: 0, fontSize: 12 }}
          onClick={() => {
            setOpen(false);
            setActivePage('skills');
          }}
        >
          {t('chat.skill.manage')}
        </Button>
      </div>
    </div>
  );

  return (
    <Popover
      trigger="click"
      placement="topLeft"
      content={content}
      arrow={false}
      open={open}
      onOpenChange={setOpen}
    >
      <Tooltip title={t('chat.skill.title')} open={open ? false : undefined}>
        <Badge count={badgeCount} size="small" offset={[-4, 4]} color={token.colorPrimary} showZero={false}>
          <Button
            type="text"
            size="small"
            aria-label={t('chat.skill.title')}
            icon={<Sparkles size={14} />}
            style={badgeCount > 0 ? { color: token.colorPrimary } : undefined}
          />
        </Badge>
      </Tooltip>
    </Popover>
  );
}
