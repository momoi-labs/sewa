import type { Item, ItemKind, Os } from '../api';

/** The key that makes an item unique in a role. */
export function itemKey(item: Pick<Item, 'kind' | 'name'>): string {
  return `${item.kind}:${item.name}`;
}

/** A key's manager prefix and its name: `brew:jq` is `brew` and `jq`. */
export function splitKey(name: string): { scope: string | null; name: string } {
  const at = name.indexOf(':');
  return at > 0 ? { scope: name.slice(0, at), name: name.slice(at + 1) } : { scope: null, name };
}

const LIMIT = 200;

/** A name or version as the controller takes it: not empty, at most 200
 *  characters, no spaces or control characters. */
export function isWord(value: string): boolean {
  return value.length > 0 && value.length <= LIMIT && !/[\s\p{Cc}]/u.test(value);
}

/** Why an item is refused, or null. */
export function itemProblem(item: Item): string | null {
  if (!isWord(item.name)) return 'a name of up to 200 characters, without spaces';
  if (!isWord(item.version)) return 'a version of up to 200 characters, without spaces';
  if (item.kind === 'tool' && item.version === 'absent') return 'latest or a version: only a package can be absent';
  return null;
}

export type Diff = { added: Item[]; removed: Item[]; changed: { before: Item; after: Item }[] };

/** What a save changes in a role's items. */
export function diffItems(before: Item[], after: Item[]): Diff {
  const old = new Map(before.map((item) => [itemKey(item), item]));
  const now = new Map(after.map((item) => [itemKey(item), item]));
  const diff: Diff = { added: [], removed: [], changed: [] };
  for (const [key, item] of now) {
    const was = old.get(key);
    if (!was) diff.added.push(item);
    else if (was.version !== item.version || (was.os ?? null) !== (item.os ?? null))
      diff.changed.push({ before: was, after: item });
  }
  for (const [key, item] of old) if (!now.has(key)) diff.removed.push(item);
  return diff;
}

export function diffSize(diff: Diff): number {
  return diff.added.length + diff.removed.length + diff.changed.length;
}

/** An item as one line: `brew:jq latest, macOS only`. */
export function describeItem(item: Item): string {
  const os = item.os === 'linux' ? ', Linux only' : item.os === 'macos' ? ', macOS only' : '';
  return `${item.name} ${item.version}${os}`;
}

/** Reads `os=linux`, `os=macos`, or `os=` (every OS) as typed on a chip. */
export function parseOs(text: string): Os | null | undefined {
  const match = /^\s*os\s*=\s*(\w*)\s*$/.exec(text);
  if (!match) return undefined;
  const value = match[1]!.toLowerCase();
  if (value === '') return null;
  if (value === 'linux') return 'linux';
  if (value === 'macos' || value === 'mac') return 'macos';
  return undefined;
}

export const kindNames: Record<ItemKind, string> = { package: 'Package', tool: 'mise tool' };
