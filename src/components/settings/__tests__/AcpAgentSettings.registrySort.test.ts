import { describe, expect, it } from 'vitest';
import type { RegistryAgent } from '@/types/acp';
import { sortRegistryAgents } from '@/lib/acpRegistrySort';

function agent(id: string, quarantineReason?: string): RegistryAgent {
  return { id, name: id, quarantineReason };
}

describe('sortRegistryAgents', () => {
  it('moves quarantined agents to the end while preserving group order', () => {
    const registry = [
      agent('quarantined-first', 'broken dependency'),
      agent('available-first'),
      agent('quarantined-second', 'startup regression'),
      agent('available-second'),
    ];

    expect(sortRegistryAgents(registry).map((item) => item.id)).toEqual([
      'available-first',
      'available-second',
      'quarantined-first',
      'quarantined-second',
    ]);
    expect(registry.map((item) => item.id)).toEqual([
      'quarantined-first',
      'available-first',
      'quarantined-second',
      'available-second',
    ]);
  });
});
