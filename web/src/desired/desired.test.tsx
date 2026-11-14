import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import type { Change, ChangeDetail, Host, HostState, Role } from '../api';
import { mockApi, renderRoute } from '../test-utils';
import { diffLines } from './compare';

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

const role: Role = {
  id: '33333333-3333-4333-8333-333333333333',
  name: 'base',
  created_at: '2026-01-01T10:00:00Z',
  items: [{ kind: 'package', name: 'git', version: 'latest' }],
  proposed_items: [
    { kind: 'package', name: 'git', version: 'latest' },
    { kind: 'package', name: 'jq', version: 'latest' },
  ],
  hosts: [{ id: host.id, name: host.name }],
  proposed_hosts: [{ id: host.id, name: host.name }],
};

const draft: Change = {
  id: '44444444-4444-4444-8444-444444444444',
  status: 'draft',
  created_at: '2026-01-02T10:00:00Z',
  created_by: 'admin',
  closed_at: null,
  edit_count: 1,
};

const detail: ChangeDetail = {
  ...draft,
  edits: [
    {
      id: 1,
      at: '2026-01-02T10:00:00Z',
      by: 'admin',
      edit: {
        op: 'role_item_set',
        role: role.id,
        kind: 'package',
        name: 'jq',
        version: 'latest',
      },
    },
  ],
  hosts: [
    {
      id: host.id,
      name: host.name,
      status: 'connected',
      current_config: '[bootstrap.packages]\ngit = "latest"\n',
      proposed_config: '[bootstrap.packages]\ngit = "latest"\njq = "latest"\n',
      plan: {
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
        stale: false,
      },
    },
  ],
};

describe('roles', () => {
  it('lists roles and marks the ones the draft edits', async () => {
    mockApi({ 'GET /api/roles': { status: 200, body: [role] } });
    renderRoute('/roles');
    const row = (await screen.findByRole('link', { name: 'base' })).closest('tr')!;
    expect(within(row).getByText('Edited in draft')).toBeInTheDocument();
    expect(screen.getByText('1 role')).toBeInTheDocument();
  });

  it('adds an item to a role in the draft', async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      [`GET /api/roles/${role.id}`]: { status: 200, body: role },
      'GET /api/changes': { status: 200, body: [draft] },
      'GET /api/hosts': { status: 200, body: [host] },
      'POST /api/changes/draft/edits': { status: 201, body: draft },
    });
    renderRoute(`/roles/${role.id}`);
    const table = await screen.findByRole('table', { name: 'Items of base' });
    expect(within(table).getByText('Added in draft')).toBeInTheDocument();
    await user.type(screen.getByRole('textbox', { name: /Name/ }), 'ripgrep');
    await user.click(screen.getByRole('button', { name: 'Add to draft' }));
    const post = calls.find((call) => call.method === 'POST');
    expect(post?.body).toEqual({
      op: 'role_item_set',
      role: role.id,
      kind: 'package',
      name: 'ripgrep',
      version: 'latest',
    });
  });
});

describe('changes', () => {
  it("shows each host's plan with removals first", async () => {
    mockApi({
      [`GET /api/changes/${draft.id}`]: { status: 200, body: detail },
    });
    renderRoute(`/changes/${draft.id}`);
    const rows = (await screen.findAllByRole('row')).filter((row) => row.dataset.action);
    expect(rows.map((row) => row.dataset.action)).toEqual(['remove', 'create']);
    expect(within(rows[0]!).getByText('Remove')).toBeInTheDocument();
    expect(screen.getByText('1 to install, 1 to remove.')).toBeInTheDocument();
  });

  it('discards a draft after a confirmation', async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      [`GET /api/changes/${draft.id}`]: { status: 200, body: detail },
      [`POST /api/changes/${draft.id}/discard`]: {
        status: 200,
        body: { ...draft, status: 'discarded' },
      },
    });
    renderRoute(`/changes/${draft.id}`);
    await user.click(await screen.findByRole('button', { name: 'Discard' }));
    const dialog = await screen.findByRole('dialog');
    expect(calls.some((call) => call.method === 'POST')).toBe(false);
    await user.click(within(dialog).getByRole('button', { name: 'Discard' }));
    expect(calls.some((call) => call.path === `/api/changes/${draft.id}/discard`)).toBe(true);
  });
});

describe('host drift', () => {
  it('shows the drift plan and the undeclared packages', async () => {
    const user = userEvent.setup();
    const state: HostState = {
      roles: [{ id: role.id, name: role.name }],
      proposed_roles: null,
      adjustments: [],
      proposed_adjustments: null,
      items: role.items,
      config: '',
      health: 'drift',
      drift: { ...detail.hosts[0]!.plan!, status: 'drift' },
      undeclared: {
        manager: 'pacman',
        packages: ['sl'],
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
    const undeclared = await screen.findByRole('table', {
      name: 'Undeclared packages',
    });
    expect(screen.getByText('1 of 3 packages', { exact: false })).toBeInTheDocument();
    expect(within(undeclared).getByText('sl')).toBeInTheDocument();
  });
});

describe('diffLines', () => {
  it('marks added and removed lines', () => {
    expect(diffLines('a\nb\nc', 'a\nc\nd')).toBe(' a\n-b\n c\n+d');
  });
});
