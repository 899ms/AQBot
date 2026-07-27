/**
 * Full Lucide icon catalog for the selection-toolbar icon picker.
 *
 * IMPORTANT: import this module ONLY via dynamic `import()` — it pulls the
 * complete icon barrel (~1600 icons) into its own async chunk. Entry chunks
 * keep their tree-shaken named imports.
 */
import { icons, type LucideProps } from 'lucide-react';
import type { ComponentType } from 'react';
import { pascalToKebab } from './lucideIconNames';

export interface LucideIconEntry {
  /** Kebab-case name — the persisted icon identifier. */
  name: string;
  Icon: ComponentType<LucideProps>;
}

let cachedEntries: LucideIconEntry[] | null = null;
let cachedByName: Map<string, ComponentType<LucideProps>> | null = null;

export function lucideIconEntries(): LucideIconEntry[] {
  cachedEntries ??= Object.entries(icons).map(([pascal, Icon]) => ({
    name: pascalToKebab(pascal),
    Icon,
  }));
  return cachedEntries;
}

export function lucideIconByName(name: string): ComponentType<LucideProps> | null {
  cachedByName ??= new Map(lucideIconEntries().map((entry) => [entry.name, entry.Icon]));
  return cachedByName.get(name) ?? null;
}
