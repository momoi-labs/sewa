import type { SyncHost } from '../api';
import { groupOf, groups, splitKey } from '../desired/items';
import { planState, type PlanState } from './sync';

export type Change = 'install' | 'update' | 'remove';

export type PackageRow = {
  key: string;
  kind: 'package' | 'tool';
  name: string;
  manager: string;
  from: string | null;
  to: string | null;
  hosts: string[];
};

const changes: Record<string, Change> = { create: 'install', update: 'update', remove: 'remove' };

export const byName = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true });

export function managerOf(kind: string, name: string): string {
  const item = { kind: kind === 'tool' ? ('tool' as const) : ('package' as const), name };
  const group = groups.find((one) => one.key === groupOf(item))!;
  return group.manager || (splitKey(name).scope ?? '');
}

export function byPackage(hosts: SyncHost[]) {
  const rows: Record<Change, Map<string, PackageRow>> = { install: new Map(), update: new Map(), remove: new Map() };
  const planned: SyncHost[] = [];
  const unplanned: { host: SyncHost; state: Exclude<PlanState, 'changes'> }[] = [];
  let unchanged = 0;
  let unknown = 0;

  for (const host of hosts) {
    const state = planState(host);
    if (state !== 'changes') {
      unplanned.push({ host, state });
      continue;
    }
    planned.push(host);
    for (const resource of host.plan?.plan?.resources ?? []) {
      const change = changes[resource.action];
      if (!change) {
        if (resource.action === 'noop') unchanged++;
        else unknown++;
        continue;
      }
      const kind = resource.id.kind === 'tool' ? 'tool' : 'package';
      const from = change === 'install' ? null : (resource.current ?? null);
      const to = change === 'remove' ? null : (resource.desired ?? null);
      const key = [kind, resource.id.name, change === 'update' ? from : '', to ?? ''].join('\u0000');
      const row = rows[change].get(key);
      if (row) row.hosts.push(host.name);
      else
        rows[change].set(key, {
          key,
          kind,
          name: resource.id.name,
          manager: managerOf(kind, resource.id.name),
          from,
          to,
          hosts: [host.name],
        });
    }
  }

  const sorted = (map: Map<string, PackageRow>) =>
    [...map.values()]
      .map((row) => ({ ...row, hosts: [...row.hosts].sort(byName) }))
      .sort(
        (a, b) =>
          a.name.localeCompare(b.name) ||
          (a.to ?? '').localeCompare(b.to ?? '') ||
          (a.from ?? '').localeCompare(b.from ?? ''),
      );
  return {
    install: sorted(rows.install),
    update: sorted(rows.update),
    remove: sorted(rows.remove),
    planned,
    unplanned,
    unchanged,
    unknown,
  };
}

export function hostsLabel(hosts: string[], planned: number, shown = 2): { text: string; more: number } {
  if (hosts.length === planned && planned > 1) return { text: `all (${planned})`, more: 0 };
  if (hosts.length <= shown + 1) return { text: hosts.join(', '), more: 0 };
  return { text: `${hosts.slice(0, shown).join(', ')} +${hosts.length - shown}`, more: hosts.length - shown };
}
