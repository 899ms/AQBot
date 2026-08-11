import type { AvatarType } from '@/stores/userProfileStore';
import { Avatar, type theme } from 'antd';
import { User } from 'lucide-react';
import React, { useCallback, useMemo } from 'react';

type ThemeToken = ReturnType<typeof theme.useToken>['token'];

interface ChatBubblePresentationOptions {
  bubbleStyle: string;
  profile: {
    avatarType: AvatarType;
    avatarValue: string;
  };
  resolvedAvatarSrc?: string | null;
  token: ThemeToken;
}

export function useChatBubblePresentation({
  bubbleStyle,
  profile,
  resolvedAvatarSrc,
  token,
}: ChatBubblePresentationOptions) {
  const userAvatar = useMemo(() => {
    const size = 32;
    if (profile.avatarType === 'emoji' && profile.avatarValue) {
      return (
        <div
          style={{
            width: size,
            height: size,
            borderRadius: '50%',
            backgroundColor: token.colorFillSecondary,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            fontSize: 16,
          }}
        >
          {profile.avatarValue}
        </div>
      );
    }
    if ((profile.avatarType === 'url' || profile.avatarType === 'file') && profile.avatarValue) {
      const src = profile.avatarType === 'file' ? resolvedAvatarSrc : profile.avatarValue;
      return <Avatar size={size} src={src} />;
    }
    return (
      <Avatar size={size} icon={<User size={16} />} style={{ backgroundColor: token.colorPrimary }} />
    );
  }, [profile, resolvedAvatarSrc, token]);

  const getBubbleVariant = useCallback(
    (isUser: boolean): {
      variant: 'filled' | 'outlined' | 'shadow' | 'borderless';
      style?: React.CSSProperties;
    } => {
      switch (bubbleStyle) {
        case 'compact':
          return { variant: 'borderless' };
        case 'minimal':
          return { variant: 'borderless', style: { padding: '4px 8px' } };
        case 'modern':
        default:
          return { variant: isUser ? 'shadow' : 'outlined' };
      }
    },
    [bubbleStyle],
  );

  return { getBubbleVariant, userAvatar };
}
