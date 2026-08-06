import { describe, expect, it } from 'vitest';
import {
  modelHasCapability,
  supportsFunctionCalling,
  supportsReasoning,
} from '@/lib/modelCapabilities';
import type { Model } from '@/types';

function model(capabilities: Model['capabilities']): Pick<Model, 'capabilities'> {
  return { capabilities };
}

describe('modelCapabilities tool support', () => {
  it('detects FunctionCalling capability', () => {
    expect(supportsFunctionCalling(model(['TextChat', 'FunctionCalling']))).toBe(true);
    expect(supportsFunctionCalling(model(['TextChat']))).toBe(false);
    expect(supportsFunctionCalling(null)).toBe(false);
    expect(supportsFunctionCalling(undefined)).toBe(false);
  });

  it('keeps reasoning helper independent of function calling', () => {
    expect(supportsReasoning(model(['TextChat', 'Reasoning']))).toBe(true);
    expect(modelHasCapability(model(['FunctionCalling']), 'FunctionCalling')).toBe(true);
  });
});
