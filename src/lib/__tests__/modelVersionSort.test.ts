import { describe, expect, it } from 'vitest';
import {
  compareModelGroupThenVersionDesc,
  compareModelVersionDesc,
  sortGroupKeysByVersionDesc,
  sortModelsByVersionDesc,
} from '../modelVersionSort';

function order(...ids: string[]): string[] {
  return sortModelsByVersionDesc(ids, (id) => id);
}

describe('compareModelVersionDesc', () => {
  it('places higher minor versions first', () => {
    expect(compareModelVersionDesc('gpt-5.4', 'gpt-5.2')).toBeLessThan(0);
    expect(order('gpt-5.2', 'gpt-5.4', 'gpt-5.3')).toEqual([
      'gpt-5.4',
      'gpt-5.3',
      'gpt-5.2',
    ]);
  });

  it('places gpt-5.6 groups ahead of gpt-5.4 / gpt-5.5', () => {
    expect(order('gpt-5.4', 'gpt-5.6', 'gpt-5.5')).toEqual([
      'gpt-5.6',
      'gpt-5.5',
      'gpt-5.4',
    ]);
  });

  it('places stable releases before preview of the same version', () => {
    expect(compareModelVersionDesc('gpt-5.4', 'gpt-5.4-preview')).toBeLessThan(0);
    expect(order('gpt-5.4-preview', 'gpt-5.4', 'gpt-5.2')).toEqual([
      'gpt-5.4',
      'gpt-5.4-preview',
      'gpt-5.2',
    ]);
  });

  it('places base model before named variants of the same version', () => {
    expect(order('gpt-5.6-sol', 'gpt-5.6-terra', 'gpt-5.6')).toEqual([
      'gpt-5.6',
      'gpt-5.6-sol',
      'gpt-5.6-terra',
    ]);
    expect(order('gpt-5.4-mini', 'gpt-5.4')).toEqual([
      'gpt-5.4',
      'gpt-5.4-mini',
    ]);
  });

  it('compares multi-segment numeric versions', () => {
    // claude-3-7 vs claude-3-5: 7 > 5
    expect(compareModelVersionDesc('claude-3-7-sonnet', 'claude-3-5-sonnet')).toBeLessThan(0);
  });

  it('handles short vs long ids without throwing', () => {
    expect(() => compareModelVersionDesc('o1-pro', 'o1')).not.toThrow();
    const sorted = order('o1', 'o1-pro');
    expect(sorted).toHaveLength(2);
    expect(sorted).toContain('o1');
    expect(sorted).toContain('o1-pro');
  });

  it('falls back to localeCompare for pure letter ids', () => {
    expect(order('zebra', 'apple')).toEqual(['apple', 'zebra']);
  });

  it('handles empty strings safely', () => {
    expect(() => compareModelVersionDesc('', 'gpt-5')).not.toThrow();
    expect(compareModelVersionDesc('', 'gpt-5')).toBeGreaterThan(0);
    expect(compareModelVersionDesc('', '')).toBe(0);
  });

  it('sorts models by getId accessor', () => {
    const models = [
      { model_id: 'gpt-5.2' },
      { model_id: 'gpt-5.4' },
      { model_id: 'gpt-4o' },
    ];
    expect(sortModelsByVersionDesc(models, (m) => m.model_id).map((m) => m.model_id)).toEqual([
      'gpt-5.4',
      'gpt-5.2',
      'gpt-4o',
    ]);
  });

  it('sorts group keys with newer versions first', () => {
    expect(sortGroupKeysByVersionDesc(['gpt-5.4', 'gpt-5.6', 'gpt-5.5'])).toEqual([
      'gpt-5.6',
      'gpt-5.5',
      'gpt-5.4',
    ]);
  });

  it('orders by group version then model version', () => {
    const rows = [
      { group: 'gpt-5.4', id: 'gpt-5.4-mini' },
      { group: 'gpt-5.6', id: 'gpt-5.6-sol' },
      { group: 'gpt-5.6', id: 'gpt-5.6' },
      { group: 'gpt-5.5', id: 'gpt-5.5' },
    ];
    const sorted = [...rows].sort(compareModelGroupThenVersionDesc);
    expect(sorted.map((r) => r.id)).toEqual([
      'gpt-5.6',
      'gpt-5.6-sol',
      'gpt-5.5',
      'gpt-5.4-mini',
    ]);
  });
});
