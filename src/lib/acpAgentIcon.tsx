import {
  createElement,
  memo,
  useEffect,
  useState,
  type ComponentType,
  type CSSProperties,
  type ReactNode,
} from 'react';
import {
  Amp,
  ByteDance,
  Claude,
  ClaudeCode,
  Cline,
  Codex,
  Copilot,
  Cursor,
  Devin,
  Gemini,
  GeminiCLI,
  GithubCopilot,
  Goose,
  Grok,
  Junie,
  KiloCode,
  Kimi,
  Kiro,
  Kwaipilot,
  Mistral,
  Nova,
  OpenAI,
  OpenCode,
  Pi,
  Qoder,
  Qwen,
  RooCode,
  Snowflake,
  Tencent,
  Trae,
  Windsurf,
  XAI,
  Zhipu,
} from '@lobehub/icons';
import { theme } from 'antd';
import { Bot } from 'lucide-react';
import { DynamicLobeIcon } from '@/components/shared/DynamicLobeIcon';
import { useResolvedAvatarSrc } from '@/hooks/useResolvedAvatarSrc';

type SizeProps = { size?: number | string; style?: CSSProperties };

/**
 * Lobe icon pack surface we actually use.
 * NOTE: Do NOT use `.Avatar` for brands that set AVATAR_BACKGROUND = AVATAR_COLOR = '#fff'
 * (Codex, Gemini) — that renders as a blank white circle.
 * Prefer Color (brand tile) → safe Avatar tile → Mono.
 * Requires @lobehub/icons ≥5.x with unique SVG fill ids (5.15+ uses React useId).
 */
type IconPack = {
  Color?: ComponentType<SizeProps>;
  Mono?: ComponentType<SizeProps>;
  /** Brand Avatar only when background is not white-on-white (e.g. Grok). */
  Avatar?: ComponentType<SizeProps>;
};

/**
 * Build an IconPack from a Lobe compound icon.
 * - Color brands: use `.Color` tile
 * - Grok-style (no Color, dark Avatar): expose Avatar as the brand tile
 * - Mono-only: default export / `.Mono`
 */
function lobePack(
  icon: ComponentType<SizeProps> & {
    Color?: ComponentType<SizeProps>;
    Mono?: ComponentType<SizeProps>;
    Avatar?: ComponentType<SizeProps>;
  },
  opts?: { preferAvatarAsColor?: boolean },
): IconPack {
  const mono = icon.Mono ?? (typeof icon === 'function' ? icon : undefined);
  if (opts?.preferAvatarAsColor && icon.Avatar) {
    return { Color: icon.Avatar, Mono: mono, Avatar: icon.Avatar };
  }
  return {
    Color: icon.Color,
    Mono: mono,
    Avatar: icon.Avatar,
  };
}

/**
 * Map ACP registry agent ids / names to LobeHub brand icons.
 * Matching is substring-based so `codex-acp`, `claude-agent-acp`, etc. work.
 * Order matters: more specific rules first (e.g. Codex before OpenAI).
 */
const AGENT_ICON_RULES: Array<{ test: RegExp; Icon: IconPack }> = [
  { test: /codex/i, Icon: lobePack(Codex as never) },
  { test: /claude.?code|claude-code/i, Icon: lobePack(ClaudeCode as never) },
  { test: /claude|anthropic/i, Icon: lobePack(Claude as never) },
  { test: /gemini.?cli|gemini-cli/i, Icon: lobePack(GeminiCLI as never) },
  { test: /gemini/i, Icon: lobePack(Gemini as never) },
  // Grok has no Color; Avatar is black tile + white mark — correct brand tile.
  // Default export is Mono (`import { Grok } from '@lobehub/icons'` → <Grok size={…} />).
  { test: /grok-?build|grok/i, Icon: lobePack(Grok as never, { preferAvatarAsColor: true }) },
  { test: /\bxai\b/i, Icon: lobePack(XAI as never) },
  { test: /opencode|open.?code/i, Icon: OpenCode as unknown as IconPack },
  { test: /cursor/i, Icon: Cursor as unknown as IconPack },
  // GithubCopilot has no Color tile — Avatar is black + white copilot mark (same as lobehub.com/icons/github-copilot).
  { test: /github.?copilot|copilot-cli/i, Icon: lobePack(GithubCopilot as never, { preferAvatarAsColor: true }) },
  { test: /copilot/i, Icon: lobePack(Copilot as never) },
  { test: /goose/i, Icon: Goose as unknown as IconPack },
  { test: /cline/i, Icon: Cline as unknown as IconPack },
  { test: /roo.?code|roocode/i, Icon: RooCode as unknown as IconPack },
  { test: /windsurf/i, Icon: Windsurf as unknown as IconPack },
  { test: /\btrae\b/i, Icon: Trae as unknown as IconPack },
  { test: /\bkiro\b/i, Icon: Kiro as unknown as IconPack },
  { test: /devin/i, Icon: Devin as unknown as IconPack },
  { test: /qoder/i, Icon: Qoder as unknown as IconPack },
  { test: /kwaipilot|kwai.?pilot/i, Icon: Kwaipilot as unknown as IconPack },
  { test: /\bpi\b|pi-acp|pi.?agent/i, Icon: Pi as unknown as IconPack },
  // Kimi Color has white paths that vanish on light UI; Avatar (lobehub.com/icons/kimi) is the brand tile.
  { test: /kimi|moonshot/i, Icon: lobePack(Kimi as never, { preferAvatarAsColor: true }) },
  { test: /qwen/i, Icon: Qwen as unknown as IconPack },
  { test: /mistral|vibe/i, Icon: Mistral as unknown as IconPack },
  { test: /\bamp\b|amp-acp/i, Icon: Amp as unknown as IconPack },
  { test: /glm|zhipu|chatglm/i, Icon: Zhipu as unknown as IconPack },
  { test: /codebuddy|tencent/i, Icon: Tencent as unknown as IconPack },
  { test: /cortex|snowflake/i, Icon: Snowflake as unknown as IconPack },
  { test: /junie/i, Icon: Junie as unknown as IconPack },
  { test: /\bkilo\b/i, Icon: KiloCode as unknown as IconPack },
  { test: /\bnova\b/i, Icon: Nova as unknown as IconPack },
  { test: /bytedance|doubao/i, Icon: ByteDance as unknown as IconPack },
  // OpenAI last so Codex never falls through here
  { test: /openai|\bgpt[-\d]|chatgpt/i, Icon: OpenAI as unknown as IconPack },
];

export function resolveAcpAgentIcon(agentId: string, agentName?: string): IconPack | null {
  const haystack = `${agentId} ${agentName ?? ''}`;
  for (const rule of AGENT_ICON_RULES) {
    if (rule.test.test(haystack)) return rule.Icon;
  }
  return null;
}

/**
 * Prefer Color (multicolor) → Mono (B/W). Never Avatar.
 * Color marks are full brand tiles (often with their own background) —
 * do not crop them into a circle or they look like blank white discs (Codex).
 */
export function renderLobeIconPack(
  pack: IconPack,
  size: number,
  monoColor?: string,
): { node: ReactNode; kind: 'color' | 'mono' } | null {
  if (pack.Color) {
    return { node: createElement(pack.Color, { size }), kind: 'color' };
  }
  if (pack.Mono) {
    return {
      node: createElement(pack.Mono, {
        size,
        style: monoColor ? { color: monoColor } : undefined,
      }),
      kind: 'mono',
    };
  }
  // Compounded default export is usually Mono
  const asFn = pack as unknown as ComponentType<SizeProps> | undefined;
  if (typeof asFn === 'function') {
    return {
      node: createElement(asFn, {
        size,
        style: monoColor ? { color: monoColor } : undefined,
      }),
      kind: 'mono',
    };
  }
  return null;
}

/** Official ACP registry CDN icons — not user customizations. */
export function isOfficialRegistryIconUrl(url: string): boolean {
  return /cdn\.agentclientprotocol\.com/i.test(url);
}

/**
 * True when the stored icon is only the auto registry CDN asset
 * (should not override brand Color, and should not show as "custom" in IconEditor).
 */
export function isAutoRegistryIcon(icon?: string | null): boolean {
  if (!icon?.trim()) return false;
  const raw = icon.trim();
  if (raw.startsWith('url:')) {
    return isOfficialRegistryIconUrl(raw.slice(4));
  }
  return isOfficialRegistryIconUrl(raw);
}

/** Encode IconEditor type+value into ConfiguredAgent.icon storage string. */
export function encodeAcpAgentIcon(type: string | null, value: string | null): string | null {
  if (!type || !value?.trim()) return null;
  const v = value.trim();
  // Never persist official registry CDN as a "custom" icon — brand Color is the source of truth.
  if (type === 'url' && isOfficialRegistryIconUrl(v)) return null;
  return `${type}:${v}`;
}

/** Decode ConfiguredAgent.icon storage into IconEditor type+value. */
export function decodeAcpAgentIcon(icon?: string | null): {
  type: string | null;
  value: string | null;
} {
  if (!icon?.trim()) return { type: null, value: null };
  const raw = icon.trim();

  // Auto registry CDN → treat as unset so UI uses brand Color consistently
  if (isAutoRegistryIcon(raw)) {
    return { type: null, value: null };
  }

  if (
    raw.startsWith('http://')
    || raw.startsWith('https://')
    || raw.startsWith('data:image/')
    || raw.startsWith('aqbot-media://')
  ) {
    return { type: 'url', value: raw };
  }
  const colon = raw.indexOf(':');
  if (colon > 0) {
    const type = raw.slice(0, colon);
    const value = raw.slice(colon + 1);
    if (['emoji', 'url', 'file', 'model_icon'].includes(type) && value) {
      if (type === 'url' && isOfficialRegistryIconUrl(value)) {
        return { type: null, value: null };
      }
      return { type, value };
    }
  }
  if (/^\p{Extended_Pictographic}/u.test(raw)) {
    return { type: 'emoji', value: raw };
  }
  return { type: 'url', value: raw };
}

function shellStyle(size: number, opts?: { round?: number | string; overflow?: 'hidden' | 'visible' }): CSSProperties {
  return {
    width: size,
    height: size,
    minWidth: size,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
    lineHeight: 0,
    borderRadius: opts?.round ?? '50%',
    overflow: opts?.overflow ?? 'hidden',
  };
}

function parseModelIconId(value: string): string {
  const idx = value.indexOf(':');
  return idx > 0 ? value.slice(idx + 1) : value;
}

function DefaultBot({
  size,
  title,
  colorFill,
  colorText,
}: {
  size: number;
  title?: string;
  colorFill: string;
  colorText: string;
}) {
  return (
    <span
      className="aqbot-conversation-model-icon"
      style={{
        ...shellStyle(size),
        backgroundColor: colorFill,
        color: colorText,
      }}
      title={title}
    >
      <Bot size={Math.round(size * 0.58)} strokeWidth={1.8} style={{ display: 'block' }} />
    </span>
  );
}

/**
 * Circular / brand tile icon for ACP agents.
 *
 * Priority:
 * 1. User custom icon (emoji / file / non-registry url / model_icon)
 * 2. Lobe brand Color (full tile, not circular-cropped) → Mono
 * 3. Official registry CDN image (only when no brand pack)
 * 4. Default Bot
 */
export const AcpAgentIcon = memo(function AcpAgentIcon({
  agentId,
  agentName,
  icon,
  size = 20,
}: {
  agentId: string;
  agentName?: string;
  /** ConfiguredAgent.icon or registry icon URL */
  icon?: string | null;
  size?: number;
}): ReactNode {
  const { token } = theme.useToken();
  const decoded = decodeAcpAgentIcon(icon);
  const [imgFailed, setImgFailed] = useState(false);
  // Reset load errors when the source icon changes
  useEffect(() => {
    setImgFailed(false);
  }, [icon, agentId]);
  const resolvedFileSrc = useResolvedAvatarSrc(
    decoded.type === 'file' ? 'file' : 'icon',
    decoded.type === 'file' ? (decoded.value ?? '') : '',
  );

  const title = agentName || agentId;

  if (decoded.type === 'emoji' && decoded.value) {
    return (
      <span
        className="aqbot-conversation-model-icon"
        style={{
          ...shellStyle(size),
          fontSize: Math.round(size * 0.62),
          backgroundColor: token.colorFillSecondary,
        }}
        title={title}
      >
        {decoded.value}
      </span>
    );
  }

  if (decoded.type === 'model_icon' && decoded.value) {
    return (
      <span
        className="aqbot-conversation-model-icon"
        style={shellStyle(size, { round: Math.max(4, Math.round(size * 0.18)), overflow: 'visible' })}
        title={title}
      >
        <DynamicLobeIcon iconId={parseModelIconId(decoded.value)} size={size} type="color" />
      </span>
    );
  }

  const customUrl =
    decoded.type === 'url' && decoded.value && !isOfficialRegistryIconUrl(decoded.value)
      ? decoded.value
      : null;

  if (customUrl && !imgFailed) {
    return (
      <span className="aqbot-conversation-model-icon" style={shellStyle(size)} title={title}>
        <img
          alt=""
          src={customUrl}
          style={{ width: size, height: size, objectFit: 'cover', display: 'block' }}
          onError={() => setImgFailed(true)}
        />
      </span>
    );
  }

  if (decoded.type === 'file' && !imgFailed && (resolvedFileSrc || decoded.value)) {
    const src = resolvedFileSrc
      ?? (decoded.value?.startsWith('data:image/') || decoded.value?.startsWith('http')
        ? decoded.value
        : undefined);
    if (src) {
      return (
        <span className="aqbot-conversation-model-icon" style={shellStyle(size)} title={title}>
          <img
            alt=""
            src={src}
            style={{ width: size, height: size, objectFit: 'cover', display: 'block' }}
            onError={() => setImgFailed(true)}
          />
        </span>
      );
    }
  }

  // Brand: Color (tile) → Mono. Never Avatar (Codex Avatar = blank white).
  const pack = resolveAcpAgentIcon(agentId, agentName);
  if (pack) {
    const rendered = renderLobeIconPack(pack, size, token.colorText);
    if (rendered) {
      if (rendered.kind === 'color') {
        // Color packs often include their own background (Codex = white tile + gradient mark).
        // Do NOT force a circular crop — that turns the tile into a blank white disc when the
        // gradient fill id collides or fails. Keep a slight round to match app avatars.
        return (
          <span
            className="aqbot-conversation-model-icon"
            style={shellStyle(size, {
              round: Math.max(4, Math.round(size * 0.18)),
              overflow: 'hidden',
            })}
            title={title}
          >
            {rendered.node}
          </span>
        );
      }
      return (
        <span
          className="aqbot-conversation-model-icon"
          style={{
            ...shellStyle(size),
            backgroundColor: token.colorFillSecondary,
            color: token.colorText,
          }}
          title={title}
        >
          {rendered.node}
        </span>
      );
    }
  }

  // Official registry CDN only when no Lobe brand pack (and img still loads)
  const rawIcon = icon?.trim() ?? '';
  const registryUrl = isOfficialRegistryIconUrl(rawIcon)
    ? rawIcon
    : rawIcon.startsWith('url:') && isOfficialRegistryIconUrl(rawIcon.slice(4))
      ? rawIcon.slice(4)
      : null;

  if (registryUrl && !imgFailed) {
    return (
      <span
        className="aqbot-conversation-model-icon"
        style={shellStyle(size, {
          round: Math.max(4, Math.round(size * 0.18)),
          overflow: 'hidden',
        })}
        title={title}
      >
        <img
          alt=""
          src={registryUrl}
          style={{ width: size, height: size, objectFit: 'contain', display: 'block' }}
          onError={() => setImgFailed(true)}
        />
      </span>
    );
  }

  return (
    <DefaultBot
      size={size}
      title={title}
      colorFill={token.colorFillSecondary}
      colorText={token.colorTextSecondary}
    />
  );
});
