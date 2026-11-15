import { describe, expect, it } from 'vitest';

import type { SyncHost } from '../api';
import { byPackage, hostsLabel } from './byPackage';

type Resource = { name: string; kind?: string; current?: string | null; desired?: string | null; action: string };

function host(name: string, resources: Resource[] | null, extra: Partial<SyncHost> = {}): SyncHost {
  return {
    id: `id-${name}`,
    name,
    os: 'linux',
    status: 'connected',
    last_seen_at: '2026-01-02T10:01:00Z',
    roles: [],
    step: 'picked',
    plan: resources && {
      config_hash: 'c1',
      plan_hash: `p-${name}`,
      exit_code: resources.some((one) => one.action !== 'noop') ? 2 : 0,
      plan: {
        resources: resources.map((one) => ({
          id: { kind: one.kind ?? 'package', name: one.name },
          current: one.current ?? null,
          desired: one.desired ?? null,
          action: one.action,
        })),
      },
      error: null,
      reported_at: '2026-01-02T10:01:00Z',
      received_at: null,
      stale: false,
    },
    result: null,
    ...extra,
  } as SyncHost;
}

describe('byPackage', () => {
  it('merges the same change on several hosts into one row', () => {
    const plan = byPackage([
      host('mac-1', [{ name: 'brew:jq', desired: 'latest', action: 'create' }]),
      host('build-1', [{ name: 'brew:jq', desired: 'latest', action: 'create' }]),
    ]);
    expect(plan.install).toHaveLength(1);
    expect(plan.install[0]).toMatchObject({ name: 'brew:jq', manager: 'Homebrew', to: 'latest', hosts: ['build-1', 'mac-1'] });
  });

  it('keeps updates with different versions apart', () => {
    const plan = byPackage([
      host('build-1', [{ name: 'node', kind: 'tool', current: '20', desired: '22', action: 'update' }]),
      host('build-2', [{ name: 'node', kind: 'tool', current: '18', desired: '22', action: 'update' }]),
      host('build-3', [{ name: 'node', kind: 'tool', current: '20', desired: '22', action: 'update' }]),
    ]);
    expect(plan.update.map((row) => [row.from, row.to, row.hosts])).toEqual([
      ['18', '22', ['build-2']],
      ['20', '22', ['build-1', 'build-3']],
    ]);
  });

  it('counts the unchanged items and puts hosts with no plan aside', () => {
    const plan = byPackage([
      host('build-1', [
        { name: 'htop', action: 'noop' },
        { name: 'sl', current: '5', action: 'remove' },
      ]),
      host('build-2', []),
      host('build-3', null, { status: 'disconnected' }),
    ]);
    expect(plan.remove).toMatchObject([{ name: 'sl', hosts: ['build-1'] }]);
    expect(plan.unchanged).toBe(1);
    expect(plan.planned.map((one) => one.name)).toEqual(['build-1']);
    expect(plan.unplanned.map((one) => [one.host.name, one.state])).toEqual([
      ['build-2', 'in_sync'],
      ['build-3', 'offline'],
    ]);
  });
});

describe('hostsLabel', () => {
  it('says all when every planned host makes the change', () => {
    expect(hostsLabel(['a', 'b', 'c'], 3)).toEqual({ text: 'all (3)', more: 0 });
  });

  it('lists a few names and counts the rest', () => {
    expect(hostsLabel(['a', 'b', 'c'], 9)).toEqual({ text: 'a, b, c', more: 0 });
    expect(hostsLabel(['a', 'b', 'c', 'd', 'e'], 9)).toEqual({ text: 'a, b +3', more: 3 });
  });
});
