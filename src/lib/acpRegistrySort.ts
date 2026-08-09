import type { RegistryAgent } from '@/types/acp';

export function sortRegistryAgents(agents: RegistryAgent[]): RegistryAgent[] {
  return [...agents].sort(
    (a, b) => Number(!!a.quarantineReason) - Number(!!b.quarantineReason),
  );
}
