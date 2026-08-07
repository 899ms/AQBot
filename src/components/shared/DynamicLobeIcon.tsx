import { memo } from 'react';
// Static barrel import — all icon components are already mostly bundled
// via providerConfig/modelConfig; marginal addition is small.
import * as LobeIcons from '@lobehub/icons/es/icons.js';

const iconsMap = LobeIcons as unknown as Record<string, any>;

interface DynamicLobeIconProps {
  iconId: string;
  size?: number;
  type?: 'color' | 'avatar' | 'mono';
}

/**
 * Renders a @lobehub/icons icon by its toc `id` (e.g., "Ai302", "OpenAI")
 * via direct component lookup, bypassing the incomplete keyword matching
 * in ProviderIcon/ModelIcon.
 *
 * Avatar is last resort: several brands (Codex, Gemini) set both
 * AVATAR_BACKGROUND and AVATAR_COLOR to #fff → blank white disc.
 */
export const DynamicLobeIcon = memo(function DynamicLobeIcon({
  iconId,
  size = 24,
  type = 'color',
}: DynamicLobeIconProps) {
  const IconModule = iconsMap[iconId];
  if (!IconModule) return <div style={{ width: size, height: size }} />;

  // Prefer Color when requested; fall back Mono → Avatar
  if (type === 'color') {
    if (IconModule.Color) return <IconModule.Color size={size} />;
    if (IconModule.Mono) return <IconModule.Mono size={size} />;
    if (typeof IconModule === 'function') return <IconModule size={size} />;
    if (IconModule.Avatar) return <IconModule.Avatar size={size} />;
    return <div style={{ width: size, height: size }} />;
  }
  if (type === 'mono') {
    if (IconModule.Mono) return <IconModule.Mono size={size} />;
    if (typeof IconModule === 'function') return <IconModule size={size} />;
    return <div style={{ width: size, height: size }} />;
  }
  // avatar mode: still prefer Color for brands with broken white-on-white Avatar
  if (type === 'avatar') {
    if (IconModule.Color) return <IconModule.Color size={size} />;
    if (IconModule.Avatar) return <IconModule.Avatar size={size} />;
    if (IconModule.Mono) return <IconModule.Mono size={size} />;
    if (typeof IconModule === 'function') return <IconModule size={size} />;
    return <div style={{ width: size, height: size }} />;
  }
  return <IconModule size={size} />;
});
