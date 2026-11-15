import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import type { Host, HostState, Plan, Role } from '../api';
import { mockApi, renderRoute } from '../test-utils';
import { diffItems } from './items';

const host: Host = {
  id: '11111111-1111-4111-8111-111111111111',
  name: 'build-1',
  os: 'linux',
  status: 'connected',
  created_at: '2026-01-01T10:00:00Z',
  enrolled_at: '2026-01-01T10:01:00Z',
  last_seen_at: new Date().toISOString(),
  agent: null,
};

const builtin: Role = {
  id: '22222222-2222-4222-8222-222222222222',
  name: 'sewa',
  builtin: true,
  created_at: '2026-01-01T10:00:00Z',
  items: [{ kind: 'package', name: 'mise', version: 'latest', os: null }],
  hosts: [],
};

const role: Role = {
  id: '33333333-3333-4333-8333-333333333333',
  name: 'base',
  builtin: false,
  created_at: '2026-01-01T10:00:00Z',
  items: [
    { kind: 'package', name: 'git', version: 'latest', os: null },
    { kind: 'package', name: 'brew:jq', version: '1.7', os: 'macos' },
  ],
  hosts: [{ id: host.id, name: host.name }],
};

const plan: Plan = {
    config_hash: 'a',
    plan_hash: 'b',
    exit_code: 2,
    plan: {
      resources: [
        {
          id: { kind: 'package', name: 'jq' },
          current: null,
          desired: 'latest',
          action: 'create',
        },
        {
          id: { kind: 'package', name: 'sl' },
          current: '5.0',
          desired: 'absent',
          action: 'remove',
        },
      ],
    },
    error: null,
    reported_at: '2026-01-02T10:01:00Z',
    received_at: null,
    stale: false,
  };

describe('roles', () => {
  it('lists roles with their OS and marks the built-in one', async () => {
    mockApi({ 'GET /api/roles': { status: 200, body: [builtin, role] } });
    renderRoute('/roles');
    const row = (await screen.findByRole('link', { name: 'sewa' })).closest('tr')!;
    expect(within(row).getByText('Built in')).toBeInTheDocument();
    const base = screen.getByRole('link', { name: 'base' }).closest('tr')!;
    expect(within(base).getByText('Linux and macOS')).toBeInTheDocument();
  });

  it('saves a role after it shows the diff', async () => {
    const user = userEvent.setup();
    const saved = { ...role, items: [...role.items, { kind: 'package', name: 'ripgrep', version: 'latest', os: null }] };
    const calls = mockApi({
      [`GET /api/roles/${role.id}`]: { status: 200, body: role },
      'GET /api/roles': { status: 200, body: [builtin, role] },
      [`PUT /api/roles/${role.id}`]: { status: 200, body: saved },
    });
    renderRoute(`/roles/${role.id}`);
    await user.type(await screen.findByRole('combobox', { name: 'Packages' }), 'ripgrep{Enter}');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    const dialog = await screen.findByRole('dialog');
    const row = within(dialog).getByText('ripgrep').closest('tr')!;
    expect(within(row).getByText('Added')).toBeInTheDocument();
    expect(calls.some((call) => call.method === 'PUT')).toBe(false);
    await user.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(calls.find((call) => call.method === 'PUT')?.body).toEqual({ name: 'base', items: saved.items }),
    );
  });
});

describe('host roles', () => {
  it('gives a role to a host at once', async () => {
    const user = userEvent.setup();
    const other: Role = { ...role, id: '44444444-4444-4444-8444-444444444444', name: 'dev', hosts: [] };
    const state: HostState = {
      roles: [{ id: builtin.id, name: builtin.name }],
      adjustments: [],
      items: builtin.items,
      config: '',
      health: null,
      drift: null,
      undeclared: null,
    };
    const calls = mockApi({
      [`GET /api/hosts/${host.id}`]: { status: 200, body: host },
      [`GET /api/hosts/${host.id}/state`]: { status: 200, body: state },
      'GET /api/roles': { status: 200, body: [builtin, other] },
      [`POST /api/hosts/${host.id}/edits`]: {
        status: 200,
        body: { ...state, roles: [...state.roles, { id: other.id, name: other.name }] },
      },
    });
    renderRoute(`/hosts/${host.id}`);
    await user.click(await screen.findByRole('tab', { name: 'Desired state' }));
    const table = await screen.findByRole('table', { name: 'Roles of build-1' });
    expect(within(table).getByText('Built in')).toBeInTheDocument();
    expect(within(table).queryByRole('button', { name: 'Take away sewa' })).toBeNull();
    await user.click(screen.getByRole('combobox', { name: 'Role to give' }));
    await user.click(await screen.findByRole('option', { name: 'dev' }));
    await user.click(screen.getByRole('button', { name: 'Give role' }));
    await waitFor(() =>
      expect(calls.find((call) => call.method === 'POST')?.body).toEqual({
        edits: [{ op: 'host_role_add', role: other.id }],
      }),
    );
    expect(await screen.findByRole('link', { name: 'dev' })).toBeInTheDocument();
  });
});

describe('host drift', () => {
  it('shows the drift plan and the undeclared packages', async () => {
    const user = userEvent.setup();
    const state: HostState = {
      roles: [{ id: role.id, name: role.name }],
      adjustments: [],
      items: role.items,
      config: '',
      health: 'drift',
      drift: {
        ...plan,
        // A plan the agent kept while offline.
        received_at: '2026-01-02T11:30:00Z',
        status: 'drift',
        ignored: false,
      },
      undeclared: {
        manager: 'pacman',
        packages: ['sl'],
        ignored_count: 0,
        explicit_count: 3,
        reported_at: '2026-01-02T10:01:00Z',
      },
    };
    mockApi({
      [`GET /api/hosts/${host.id}`]: { status: 200, body: host },
      [`GET /api/hosts/${host.id}/state`]: { status: 200, body: state },
    });
    renderRoute(`/hosts/${host.id}`);
    await user.click(await screen.findByRole('tab', { name: 'Drift' }));
    const rows = (await screen.findAllByRole('row')).filter((row) => row.dataset.action);
    expect(rows.map((row) => row.dataset.action)).toEqual(['remove', 'create']);
    expect(screen.getByText('1 to install, 1 to remove.')).toBeInTheDocument();
    const undeclared = await screen.findByRole('table', {
      name: 'Undeclared packages',
    });
    expect(screen.getByText('1 of 3 packages', { exact: false })).toBeInTheDocument();
    expect(within(undeclared).getByText('sl')).toBeInTheDocument();
    expect(screen.getByText(/kept by the agent while offline/)).toBeInTheDocument();
  });
});

describe('diffItems', () => {
  it('lists added, removed, and changed items', () => {
    const before = role.items;
    const after = [
      { ...before[1]!, os: null },
      { kind: 'tool' as const, name: 'node', version: '22', os: null },
    ];
    const diff = diffItems(before, after);
    expect(diff.added.map((item) => item.name)).toEqual(['node']);
    expect(diff.removed.map((item) => item.name)).toEqual(['git']);
    expect(diff.changed.map((change) => change.after.name)).toEqual(['brew:jq']);
  });
});
