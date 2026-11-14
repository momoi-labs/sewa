import type { Item, ItemKind, Named } from '../api';

export type Difference = 'added' | 'removed' | 'changed' | 'same';

export type ItemRow = {
  kind: ItemKind;
  name: string;
  /** The approved version, or null when the item is not approved. */
  before: string | null;
  /** The version with the draft applied, or null when the draft drops it. */
  after: string | null;
  difference: Difference;
};

function difference(before: unknown, after: unknown): Difference {
  if (before === null) return 'added';
  if (after === null) return 'removed';
  return before === after ? 'same' : 'changed';
}

/** The approved items beside the items with the draft applied, by kind and name. */
export function compareItems(approved: Item[], proposed: Item[] | null): ItemRow[] {
  const key = (item: Item) => `${item.kind}\u0000${item.name}`;
  const rows = new Map<string, ItemRow>();
  for (const item of approved) {
    rows.set(key(item), {
      kind: item.kind,
      name: item.name,
      before: item.version,
      after: item.version,
      difference: 'same',
    });
  }
  if (proposed) {
    for (const row of rows.values()) row.after = null;
    for (const item of proposed) {
      const row = rows.get(key(item));
      if (row) row.after = item.version;
      else
        rows.set(key(item), {
          kind: item.kind,
          name: item.name,
          before: null,
          after: item.version,
          difference: 'same',
        });
    }
  }
  return [...rows.values()]
    .map((row) => ({ ...row, difference: difference(row.before, row.after) }))
    .sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
}

export type NamedRow = Named & { difference: Difference };

/** Approved roles or hosts beside the ones with the draft applied. */
export function compareNamed(approved: Named[], proposed: Named[] | null): NamedRow[] {
  const after = new Set((proposed ?? approved).map((entry) => entry.id));
  const before = new Set(approved.map((entry) => entry.id));
  const all = new Map([...approved, ...(proposed ?? [])].map((entry) => [entry.id, entry]));
  return [...all.values()]
    .map((entry) => ({
      ...entry,
      difference: difference(before.has(entry.id) || null, after.has(entry.id) || null),
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** A line diff of two configs, with `+`, `-` or a space before each line,
 *  as CodeBlock's `diff` reads it. Configs are short, so a plain LCS does. */
export function diffLines(before: string, after: string): string {
  const a = before.split('\n');
  const b = after.split('\n');
  // lcs(i, j): the length of the longest common run of a[i..] and b[j..].
  const width = b.length + 1;
  const table = new Array<number>((a.length + 1) * width).fill(0);
  const lcs = (i: number, j: number) => table[i * width + j] ?? 0;
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      table[i * width + j] = a[i] === b[j] ? lcs(i + 1, j + 1) + 1 : Math.max(lcs(i + 1, j), lcs(i, j + 1));
    }
  }
  const lines: string[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      lines.push(` ${a[i] ?? ''}`);
      i++;
      j++;
    } else if (j < b.length && (i === a.length || lcs(i, j + 1) >= lcs(i + 1, j))) {
      lines.push(`+${b[j] ?? ''}`);
      j++;
    } else {
      lines.push(`-${a[i] ?? ''}`);
      i++;
    }
  }
  return lines.join('\n');
}
