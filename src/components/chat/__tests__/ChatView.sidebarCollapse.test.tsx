import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

describe('ChatView sidebar collapse control', () => {
  const readSource = () => fs.readFileSync(
    path.resolve(process.cwd(), 'src/components/chat/ChatView.tsx'),
    'utf8',
  );

  it('renders the sidebar toggle before the active conversation icon in the top bar', () => {
    const source = readSource();
    const topBarIndex = source.indexOf('{/* Top Bar */}');
    const toggleIndex = source.indexOf('{renderChatSidebarToggle()}', topBarIndex);
    const iconIndex = source.indexOf('{renderConvIconForChat(24)}', topBarIndex);

    expect(toggleIndex).toBeGreaterThan(topBarIndex);
    expect(toggleIndex).toBeLessThan(iconIndex);
  });

  it('renders the sidebar toggle before the welcome title when no conversation is active', () => {
    const source = readSource();
    const activeIconIndex = source.indexOf('{renderConvIconForChat(24)}');
    const inactiveToggleIndex = source.indexOf('{renderChatSidebarToggle()}', activeIconIndex + 1);
    const welcomeIndex = source.indexOf("<Typography.Text type=\"secondary\">{t('chat.welcome')}</Typography.Text>");

    expect(inactiveToggleIndex).toBeGreaterThan(activeIconIndex);
    expect(inactiveToggleIndex).toBeLessThan(welcomeIndex);
  });

  it('persists the inverse chat_sidebar_collapsed setting from the title bar button', () => {
    const source = readSource();

    expect(source).toContain('PanelLeftClose');
    expect(source).toContain('PanelLeftOpen');
    expect(source).toContain('const saveSettings = useSettingsStore((s) => s.saveSettings);');
    expect(source).toContain('saveSettings({ chat_sidebar_collapsed: !chatSidebarCollapsed })');
  });

  it('does not render a tooltip for the title bar sidebar toggle', () => {
    const source = readSource();
    const toggleStart = source.indexOf('const renderChatSidebarToggle = useCallback(() => {');
    const toggleEnd = source.indexOf('const { getBubbleVariant, userAvatar }', toggleStart);
    const toggleSource = source.slice(toggleStart, toggleEnd);

    expect(toggleSource).not.toContain('<Tooltip');
    expect(toggleSource).toContain('aria-label={label}');
  });

  it('constrains long active conversation titles in the top bar', () => {
    const source = readSource();

    expect(source).toContain('aqbot-chat-title-shell');
    expect(source).toContain('aqbot-chat-title-text');
    expect(source).toContain('ellipsis={{ tooltip: activeConversation.title }}');
  });
});
