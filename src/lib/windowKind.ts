export type FrontendKind = 'main' | 'selection-toolbar';

export function frontendKindForWindow(label: string): FrontendKind {
  return label === 'selection-toolbar' ? 'selection-toolbar' : 'main';
}
