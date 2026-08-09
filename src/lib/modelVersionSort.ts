/**
 * Numeric-aware model id/name sorting: newer versions first within a series.
 * e.g. gpt-5.4 before gpt-5.2; stable releases before preview/beta of the same version.
 */

const PRE_RELEASE_MARKERS = new Set([
  'preview',
  'beta',
  'alpha',
  'rc',
  'dev',
  'nightly',
  'snapshot',
  'experimental',
  'draft',
]);

type Token = { kind: 'num'; value: number } | { kind: 'text'; value: string };

function tokenizeModelId(id: string): Token[] {
  const normalized = id.trim().toLowerCase();
  if (!normalized) return [];

  const tokens: Token[] = [];
  const re = /(\d+(?:\.\d+)*)|([a-z]+)|([^a-z0-9]+)/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(normalized)) !== null) {
    if (match[1] != null) {
      // Split multi-part numbers like "3.5" into separate numeric tokens
      for (const part of match[1].split('.')) {
        if (part.length === 0) continue;
        tokens.push({ kind: 'num', value: Number(part) });
      }
    } else if (match[2] != null) {
      tokens.push({ kind: 'text', value: match[2] });
    }
    // separators ignored
  }
  return tokens;
}

function isPreReleaseToken(token: Token): boolean {
  return token.kind === 'text' && PRE_RELEASE_MARKERS.has(token.value);
}

function hasPreRelease(tokens: Token[]): boolean {
  return tokens.some(isPreReleaseToken);
}

/**
 * Compare two model identifiers for display order.
 * Returns < 0 if `a` should appear before `b` (newer first).
 */
export function compareModelVersionDesc(a: string, b: string): number {
  const left = tokenizeModelId(a);
  const right = tokenizeModelId(b);

  if (left.length === 0 && right.length === 0) {
    return a.localeCompare(b);
  }
  if (left.length === 0) return 1;
  if (right.length === 0) return -1;

  const max = Math.max(left.length, right.length);
  for (let i = 0; i < max; i++) {
    const lt = left[i];
    const rt = right[i];

    if (!lt && rt) {
      // Left is shorter (base id). Prefer stable base before pre-release / variants.
      // e.g. gpt-5.4 before gpt-5.4-preview; gpt-5.6 before gpt-5.6-sol.
      if (hasPreRelease(right) && !hasPreRelease(left)) return -1;
      if (hasPreRelease(left) && !hasPreRelease(right)) return 1;
      return -1; // shorter (base) first
    }
    if (lt && !rt) {
      // Left is longer.
      if (hasPreRelease(left) && !hasPreRelease(right)) return 1;
      if (hasPreRelease(right) && !hasPreRelease(left)) return -1;
      return 1; // longer (variant) after base
    }
    if (!lt || !rt) break;

    if (lt.kind === 'num' && rt.kind === 'num') {
      if (lt.value !== rt.value) return rt.value - lt.value; // higher first
      continue;
    }

    if (lt.kind === 'num' && rt.kind === 'text') {
      // Prefer numeric continuation as "more versioned" / often newer path
      return -1;
    }
    if (lt.kind === 'text' && rt.kind === 'num') {
      return 1;
    }

    // both text
    if (lt.kind === 'text' && rt.kind === 'text') {
      const lPre = isPreReleaseToken(lt);
      const rPre = isPreReleaseToken(rt);
      if (lPre !== rPre) {
        return lPre ? 1 : -1; // stable before pre-release
      }
      if (lt.value !== rt.value) {
        return lt.value.localeCompare(rt.value);
      }
    }
  }

  // Same token stream quality — prefer stable over pre-release if one has markers later
  const lPre = hasPreRelease(left);
  const rPre = hasPreRelease(right);
  if (lPre !== rPre) return lPre ? 1 : -1;

  return a.localeCompare(b);
}

export function sortModelsByVersionDesc<T>(
  items: T[],
  getId: (item: T) => string,
): T[] {
  return [...items].sort((a, b) => compareModelVersionDesc(getId(a), getId(b)));
}

/**
 * Sort by group version desc (newer groups first), then model id version desc within group.
 */
export function compareModelGroupThenVersionDesc(
  a: { group: string; id: string },
  b: { group: string; id: string },
): number {
  const groupCmp = compareModelVersionDesc(a.group, b.group);
  if (groupCmp !== 0) return groupCmp;
  return compareModelVersionDesc(a.id, b.id);
}

/** Sort group keys with newer version groups first. */
export function sortGroupKeysByVersionDesc(groupKeys: string[]): string[] {
  return sortModelsByVersionDesc(groupKeys, (key) => key);
}
