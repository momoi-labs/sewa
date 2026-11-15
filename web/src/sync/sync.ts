import type { EditEntry, Sync, SyncHost } from '../api';

function count(value: unknown, one: string, many: string): string | null {
  const n = Array.isArray(value) ? value.length : 0;
  if (n === 0) return null;
  return `${n} ${n === 1 ? one : many}`;
}

function names(value: unknown): string {
  return Array.isArray(value) ? value.join(', ') : '';
}

/** One edit as a sentence, such as "base: 2 items added". */
export function describeEdit(edit: EditEntry): string {
  const data = edit.data;
  const host = edit.host?.name ?? 'a host';
  switch (edit.kind) {
    case 'role.saved': {
      const parts = [
        typeof data.renamed_from === 'string' ? `renamed from ${data.renamed_from}` : null,
        count(data.added, 'item added', 'items added'),
        count(data.changed, 'item changed', 'items changed'),
        count(data.removed, 'item removed', 'items removed'),
      ].filter(Boolean);
      return `Role ${edit.role?.name ?? data.name}: ${parts.join(', ') || 'saved'}`;
    }
    case 'host.roles_set': {
      const parts = [
        names(data.added) && `gets ${names(data.added)}`,
        names(data.removed) && `loses ${names(data.removed)}`,
      ].filter(Boolean);
      return `${host} ${parts.join(' and ')}`;
    }
    case 'host.items_set':
      return `${host}: ${count(data.edits, 'adjustment', 'adjustments') ?? 'adjustments'} changed`;
    case 'drift.adopted':
      return `${host}: ${String(data.package)} adopted into ${typeof data.role === 'string' ? 'a role' : 'its adjustments'}`;
    default:
      return edit.kind;
  }
}

export type PlanState = 'changes' | 'in_sync' | 'failed' | 'offline' | 'planning';

/** What a picked host's plan says. */
export function planState(host: SyncHost): PlanState {
  if (host.plan && !host.plan.stale) {
    if (host.plan.error || host.plan.exit_code === 1) return 'failed';
    return host.plan.exit_code === 0 ? 'in_sync' : 'changes';
  }
  return host.status === 'connected' ? 'planning' : 'offline';
}

/** The hosts in each step, by name. */
export function tally(sync: Sync) {
  const of = (...steps: SyncHost['step'][]) => sync.hosts.filter((host) => steps.includes(host.step));
  return {
    picked: of('picked'),
    inSync: of('in_sync'),
    skipped: of('skipped'),
    waiting: of('waiting'),
    applied: of('applied'),
    failed: of('failed', 'replan'),
  };
}

/** When the last host reported, or when the sync started. */
export function finishedAt(sync: Sync): string {
  let last = sync.approvals.at(-1)?.created_at ?? sync.created_at;
  for (const host of sync.hosts) {
    const at = host.result?.received_at ?? host.result?.reported_at;
    if (at && Date.parse(at) > Date.parse(last)) last = at;
  }
  return last;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
