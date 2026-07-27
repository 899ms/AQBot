import { describe, expect, it } from 'vitest';
import { icons } from 'lucide-react';
import { kebabToPascal, pascalToKebab } from '../lucideIconNames';

describe('lucide icon name conversions', () => {
  it('produces canonical kebab names for representative icons', () => {
    expect(pascalToKebab('WandSparkles')).toBe('wand-sparkles');
    expect(pascalToKebab('AArrowDown')).toBe('a-arrow-down');
    expect(pascalToKebab('Axis3d')).toBe('axis-3d');
    expect(kebabToPascal('list-collapse')).toBe('ListCollapse');
  });

  it('round-trips every icon in the lucide barrel', () => {
    // The picker stores pascalToKebab(name); rendering resolves back through
    // the same mapping — the round trip must be lossless for the entire set.
    for (const pascal of Object.keys(icons)) {
      expect(kebabToPascal(pascalToKebab(pascal)), pascal).toBe(pascal);
    }
  });
});
