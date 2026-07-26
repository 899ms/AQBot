import { describe, expect, it } from 'vitest';
import { frontendKindForWindow } from '../windowKind';

describe('frontendKindForWindow', () => {
  it('routes only the selection-toolbar label to the lightweight frontend', () => {
    expect(frontendKindForWindow('selection-toolbar')).toBe('selection-toolbar');
    expect(frontendKindForWindow('main')).toBe('main');
    expect(frontendKindForWindow('other')).toBe('main');
  });
});
