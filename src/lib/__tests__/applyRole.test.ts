import { describe, expect, it } from 'vitest';
import { buildApplyRoleUpdate, roleSkillNames } from '../applyRole';
import type { Role } from '@/types';

function makeRole(overrides: Partial<Role> = {}): Role {
  return {
    id: 'role-1',
    name: 'Demo',
    description: null,
    system_prompt: 'You are helpful',
    opening_message: null,
    opening_questions: [],
    tags: [],
    avatar: null,
    avatar_type: null,
    avatar_value: null,
    temperature: 0.3,
    top_p: 0.9,
    enabled_mcp_server_ids: [],
    enabled_skill_names: [],
    source_kind: 'local',
    source_ref: null,
    created_at: 1,
    updated_at: 1,
    ...overrides,
  };
}

describe('buildApplyRoleUpdate', () => {
  it('always sets prompt, params, and role mode', () => {
    const update = buildApplyRoleUpdate(makeRole());
    expect(update).toEqual({
      system_prompt: 'You are helpful',
      temperature: 0.3,
      top_p: 0.9,
      mode: 'role',
    });
  });

  it('writes mcp ids only when the role list is non-empty', () => {
    const empty = buildApplyRoleUpdate(makeRole({ enabled_mcp_server_ids: [] }));
    expect(empty.enabled_mcp_server_ids).toBeUndefined();

    const filled = buildApplyRoleUpdate(
      makeRole({ enabled_mcp_server_ids: ['mcp-a', 'mcp-b'] }),
    );
    expect(filled.enabled_mcp_server_ids).toEqual(['mcp-a', 'mcp-b']);
  });

  it('can skip mcp application', () => {
    const update = buildApplyRoleUpdate(
      makeRole({ enabled_mcp_server_ids: ['mcp-a'] }),
      { applyMcp: false },
    );
    expect(update.enabled_mcp_server_ids).toBeUndefined();
  });
});

describe('roleSkillNames', () => {
  it('trims and drops empty skill names', () => {
    expect(roleSkillNames(makeRole({
      enabled_skill_names: ['  a  ', '', 'b'],
    }))).toEqual(['a', 'b']);
  });
});
