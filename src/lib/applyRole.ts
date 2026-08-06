import { CONV_ICON_KEY } from '@/lib/convIcon';
import { saveRoleIntro } from '@/lib/roleIntro';
import type { Role, UpdateConversationInput } from '@/types';

export const CONV_ROLE_ID_KEY = (conversationId: string) => `aqbot_conv_role_${conversationId}`;

export function getConversationRoleId(conversationId: string): string | null {
  try {
    return localStorage.getItem(CONV_ROLE_ID_KEY(conversationId));
  } catch {
    return null;
  }
}

export function setConversationRoleId(conversationId: string, roleId: string | null) {
  try {
    if (roleId) {
      localStorage.setItem(CONV_ROLE_ID_KEY(conversationId), roleId);
    } else {
      localStorage.removeItem(CONV_ROLE_ID_KEY(conversationId));
    }
  } catch {
    // ignore storage failures
  }
}

function getRoleAvatar(role: Pick<Role, 'avatar' | 'avatar_type' | 'avatar_value'>) {
  const value = role.avatar_value ?? role.avatar ?? '';
  const type =
    role.avatar_type
    ?? (value
      ? (value.startsWith('http://') || value.startsWith('https://') ? 'url' : 'emoji')
      : null);
  return { type, value };
}

/** Persist avatar icon + opening intro for a conversation after a role is applied. */
export function syncConversationRoleMetadata(conversationId: string, role: Role) {
  const avatar = getRoleAvatar(role);
  try {
    if (avatar.type && avatar.value) {
      localStorage.setItem(
        CONV_ICON_KEY(conversationId),
        JSON.stringify({ type: avatar.type, value: avatar.value }),
      );
    } else {
      localStorage.removeItem(CONV_ICON_KEY(conversationId));
    }
  } catch {
    // ignore
  }
  saveRoleIntro(conversationId, role);
  setConversationRoleId(conversationId, role.id);
}

export interface BuildApplyRoleUpdateOptions {
  /** When true (default), write MCP ids if the role defines a non-empty list. */
  applyMcp?: boolean;
  /** When true (default), the caller should enable listed skills globally. */
  applySkills?: boolean;
}

/**
 * Build the conversation update payload for applying a role.
 *
 * Empty capability lists do not clear existing conversation MCP/skill settings.
 */
export function buildApplyRoleUpdate(
  role: Role,
  options: BuildApplyRoleUpdateOptions = {},
): UpdateConversationInput {
  const applyMcp = options.applyMcp !== false;
  const update: UpdateConversationInput = {
    system_prompt: role.system_prompt,
    temperature: role.temperature,
    top_p: role.top_p,
    mode: 'role',
  };

  const mcpIds = role.enabled_mcp_server_ids ?? [];
  if (applyMcp && mcpIds.length > 0) {
    update.enabled_mcp_server_ids = [...mcpIds];
  }

  return update;
}

export function roleSkillNames(role: Role): string[] {
  return (role.enabled_skill_names ?? []).map((name) => name.trim()).filter(Boolean);
}
