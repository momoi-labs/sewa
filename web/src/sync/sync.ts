import type { EditEntry, HostPlan, Sync, SyncHost } from '../api';

function count(value: unknown, one: string, many: string): string | null {
  const n = Array.isArray(value) ? value.length : 0;
  if (n === 0) return null;
  return `${n} ${n === 1 ? one : many}`;
}

function names(value: unknown): string {
  return Array.isArray(value) ? value.join(', ') : '';
}

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

export function planState(host: SyncHost): PlanState {
  if (host.plan && !host.plan.stale) {
    if (host.plan.exit_code === 2) return 'changes';
    return host.plan.exit_code === 0 && !host.plan.error ? 'in_sync' : 'failed';
  }
  return host.status === 'connected' ? 'planning' : 'offline';
}

export function sameTargets(sync: Sync, targets: HostPlan[], only?: string[]): boolean {
  const shown = sync.hosts.filter(
    (host) =>
      (!only || only.includes(host.id)) &&
      ['picked', 'failed', 'replan'].includes(host.step) &&
      planState(host) === 'changes',
  );
  return (
    shown.length === targets.length &&
    targets.every((target) => {
      const plan = shown.find((host) => host.id === target.host_id)?.plan;
      return plan?.config_hash === target.config_hash && plan.plan_hash === target.plan_hash;
    })
  );
}

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

export type HostFilter = 'failed' | 'waiting' | 'applied' | 'in_sync' | 'skipped';

export const hostFilters: { key: HostFilter; label: string }[] = [
  { key: 'failed', label: 'Failed' },
  { key: 'waiting', label: 'Waiting' },
  { key: 'applied', label: 'Applied' },
  { key: 'in_sync', label: 'In sync' },
  { key: 'skipped', label: 'Skipped' },
];

export function filterOf(step: SyncHost['step']): HostFilter | null {
  switch (step) {
    case 'failed':
    case 'replan':
      return 'failed';
    case 'picked':
      return null;
    default:
      return step;
  }
}

const stepRank: Record<SyncHost['step'], number> = {
  failed: 0,
  replan: 1,
  waiting: 2,
  picked: 3,
  applied: 4,
  in_sync: 5,
  skipped: 6,
};

export function orderHosts(hosts: SyncHost[]): SyncHost[] {
  return [...hosts].sort(
    (a, b) => stepRank[a.step] - stepRank[b.step] || a.name.localeCompare(b.name, undefined, { numeric: true }),
  );
}

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
