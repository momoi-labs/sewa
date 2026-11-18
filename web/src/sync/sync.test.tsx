import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { focusManager } from '@tanstack/react-query';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ApprovalOptions, Host, HostState, ListedHost, Passkey, Sync, SyncHost } from '../api';
import { mockApi, renderRoute } from '../test-utils';
import { decode, encode } from '../webauthn';
import { activity, orderHosts } from './sync';

const host: Host = {
  id: '11111111-1111-4111-8111-111111111111',
  name: 'build-1',
  os: 'linux',
  status: 'connected',
  created_at: '2026-01-01T10:00:00Z',
  disabled_at: null,
  enrolled_at: '2026-01-01T10:01:00Z',
  last_seen_at: new Date().toISOString(),
  agent: null,
};

const plan = {
  config_hash: 'c1',
  plan_hash: 'p1',
  exit_code: 2,
  plan: { resources: [{ id: { kind: 'package', name: 'jq' }, current: null, desired: 'latest', action: 'create' }] },
  error: null,
  reported_at: '2026-01-02T10:01:00Z',
  received_at: null,
  stale: false,
};

function picked(id: string, name: string, extra: Partial<SyncHost> = {}): SyncHost {
  return {
    id,
    name,
    os: 'linux',
    status: 'connected',
    last_seen_at: plan.reported_at,
    roles: [],
    step: 'picked',
    plan,
    result: null,
    progress: null,
    ...extra,
  };
}

const ready: Sync = {
  id: '44444444-4444-4444-8444-444444444444',
  created_at: '2026-01-02T10:00:00Z',
  created_by: 'admin',
  status: 'ready',
  hosts: [picked(host.id, host.name)],
  approvals: [],
};

const options: ApprovalOptions = {
  challenge: encode(new Uint8Array([1, 2, 3])),
  rp_id: 'controller.example',
  allow_credentials: [encode(new Uint8Array([9]))],
  signed_text: 'sewa approval v1\n',
  approval_id: '55555555-5555-4555-8555-555555555555',
  change_id: ready.id,
  hosts: [{ host_id: host.id, instance_uid: 'u', config_hash: 'c1', plan_hash: 'p1' }],
};

const realCredentials = Object.getOwnPropertyDescriptor(navigator, 'credentials');

const rows = () => within(screen.getByRole('list', { name: 'Hosts' })).getAllByRole('button').map((row) => row.textContent);
const openHosts = async () => userEvent.click(await screen.findByRole('tab', { name: /^Hosts/ }));

afterEach(() => {
  vi.unstubAllGlobals();
  if (realCredentials) Object.defineProperty(navigator, 'credentials', realCredentials);
  else Reflect.deleteProperty(navigator, 'credentials');
});

function authenticator() {
  vi.stubGlobal('PublicKeyCredential', class {});
  const get = vi.fn(async () => ({
    rawId: new Uint8Array([9]).buffer,
    response: {
      authenticatorData: new Uint8Array([4]).buffer,
      clientDataJSON: new Uint8Array([5]).buffer,
      signature: new Uint8Array([6]).buffer,
    },
  }));
  const create = vi.fn(async () => ({
    rawId: new Uint8Array([7]).buffer,
    response: {
      clientDataJSON: new Uint8Array([8]).buffer,
      getPublicKey: () => new Uint8Array([10]).buffer,
      getPublicKeyAlgorithm: () => -7,
      getAuthenticatorData: () => new Uint8Array([11]).buffer,
    },
  }));
  Object.defineProperty(navigator, 'credentials', { configurable: true, value: { get, create } });
  return { get, create };
}

describe('sync', () => {
  it('starts a sync of the picked hosts', async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      'GET /api/hosts': {
        status: 200,
        body: [
          { ...host, roles: [] },
          { ...host, roles: [], id: '22222222-2222-4222-8222-222222222222', name: 'build-2', os: 'macos' },
        ],
      },
      'GET /api/syncs': { status: 200, body: [] },
      'GET /api/syncs/edits': { status: 200, body: { since: null, edits: [] } },
      'POST /api/syncs': { status: 201, body: ready },
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: ready },
    });
    renderRoute('/sync');
    await user.click(await screen.findByRole('button', { name: 'All Linux' }));
    await user.click(screen.getByRole('button', { name: 'Plan 1 host' }));
    await waitFor(() => expect(calls.find((call) => call.method === 'POST')?.body).toEqual({ hosts: [host.id] }));
    expect(await screen.findByRole('button', { name: 'Sign and sync' })).toBeInTheDocument();
  });

  describe('host picker', () => {
    const listed = (name: string, extra: Partial<ListedHost> = {}): ListedHost => ({
      ...host,
      id: `id-${name}`,
      name,
      roles: [],
      ...extra,
    });
    const web = { id: 'role-web', name: 'web' };
    const db = { id: 'role-db', name: 'db' };
    const fleet = [
      listed('build-10', { roles: [web] }),
      listed('build-2', { roles: [web, db] }),
      listed('build-1', { roles: [db], status: 'disconnected' }),
      listed('mac-1', { os: 'macos', roles: [web] }),
      listed('mac-2', { os: 'macos', status: 'disconnected' }),
    ];
    const open = async () => {
      mockApi({
        'GET /api/hosts': { status: 200, body: fleet },
        'GET /api/syncs': { status: 200, body: [] },
        'GET /api/syncs/edits': { status: 200, body: { since: null, edits: [] } },
      });
      renderRoute('/sync');
      await screen.findByRole('table', { name: 'Hosts to sync' });
    };
    const names = () =>
      within(screen.getByRole('table', { name: 'Hosts to sync' }))
        .getAllByRole('row')
        .slice(1)
        .map((row) => within(row).getAllByRole('cell')[1]!.textContent);
    const filter = (label: string) => screen.getByRole('checkbox', { name: new RegExp(`^${label}`) });

    it('lists the hosts in natural order with roles and counts', async () => {
      await open();
      expect(names()).toEqual(['build-1', 'build-2', 'build-10', 'mac-1', 'mac-2']);
      expect(screen.getByText('5 of 5 selected')).toBeInTheDocument();
      expect(filter('web')).toHaveAccessibleName(/^web\s*3$/);
      expect(screen.getByText('web, db')).toBeInTheDocument();
    });

    it('combines values of a group with OR and groups with AND', async () => {
      const user = userEvent.setup();
      await open();
      await user.click(filter('web'));
      await user.click(filter('db'));
      expect(names()).toEqual(['build-1', 'build-2', 'build-10', 'mac-1']);
      await user.click(filter('Disconnected'));
      expect(names()).toEqual(['build-1']);
      await user.click(filter('macOS'));
      expect(screen.queryByRole('table', { name: 'Hosts to sync' })).not.toBeInTheDocument();
      expect(screen.getByText('No host matches the search and the filters.')).toBeInTheDocument();
    });

    it('acts on the shown hosts only', async () => {
      const user = userEvent.setup();
      await open();
      await user.click(screen.getByRole('button', { name: 'None' }));
      expect(screen.getByText('0 of 5 selected')).toBeInTheDocument();
      await user.type(screen.getByRole('searchbox', { name: 'Filter hosts by name' }), 'build');
      await user.click(screen.getByRole('button', { name: 'All' }));
      expect(screen.getByText('3 of 5 selected')).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'All macOS' }));
      expect(screen.getByText('0 of 5 selected')).toBeInTheDocument();
      await user.clear(screen.getByRole('searchbox', { name: 'Filter hosts by name' }));
      expect(screen.getByRole('checkbox', { name: 'mac-1' })).not.toBeChecked();
    });

    it('shows a host without an OS as unknown', async () => {
      const user = userEvent.setup();
      mockApi({
        'GET /api/hosts': { status: 200, body: [listed('new-1', { os: null }), listed('build-1')] },
        'GET /api/syncs': { status: 200, body: [] },
        'GET /api/syncs/edits': { status: 200, body: { since: null, edits: [] } },
      });
      renderRoute('/sync');
      const table = await screen.findByRole('table', { name: 'Hosts to sync' });
      expect(within(table).getByRole('img', { name: 'The agent reports the OS when it connects' })).toBeInTheDocument();
      await user.click(screen.getByRole('button', { name: 'OS' }));
      expect(names()).toEqual(['build-1', 'new-1']);
      await user.click(filter('Unknown'));
      expect(names()).toEqual(['new-1']);
      await user.click(filter('Unknown'));
      await user.click(screen.getByRole('button', { name: 'None' }));
      await user.click(screen.getByRole('button', { name: 'All Linux' }));
      expect(screen.getByRole('checkbox', { name: 'new-1' })).not.toBeChecked();
      expect(screen.getByRole('checkbox', { name: 'build-1' })).toBeChecked();
    });

    it('sorts by a column and reverses on a second click', async () => {
      const user = userEvent.setup();
      await open();
      await user.click(screen.getByRole('button', { name: 'Status' }));
      expect(names()).toEqual(['build-1', 'mac-2', 'build-2', 'build-10', 'mac-1']);
      await user.click(screen.getByRole('button', { name: 'Status' }));
      expect(names()).toEqual(['mac-1', 'build-10', 'build-2', 'mac-2', 'build-1']);
      expect(screen.getByRole('columnheader', { name: 'Status' })).toHaveAttribute('aria-sort', 'descending');
    });

    it('shows the mixed state in the header checkbox', async () => {
      const user = userEvent.setup();
      await open();
      const header = screen.getByRole('checkbox', { name: 'Select the shown hosts' });
      expect(header).toBeChecked();
      await user.click(screen.getByRole('checkbox', { name: 'mac-1' }));
      expect(header).toHaveAttribute('aria-checked', 'mixed');
      await user.click(header);
      expect(header).toBeChecked();
      await user.click(header);
      expect(screen.getByText('0 of 5 selected')).toBeInTheDocument();
    });
  });

  it('signs the plans with a passkey and sends the assertion', async () => {
    const user = userEvent.setup();
    const { get } = authenticator();
    const calls = mockApi({
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: ready },
      [`POST /api/syncs/${ready.id}/approval`]: { status: 200, body: options },
      [`POST /api/syncs/${ready.id}/approve`]: {
        status: 200,
        body: { ...ready, status: 'applying', hosts: [picked(host.id, host.name, { step: 'waiting' })] },
      },
    });
    renderRoute(`/sync/${ready.id}`);
    await user.click(await screen.findByRole('button', { name: 'Sign and sync' }));
    await waitFor(() => expect(calls.some((call) => call.path.endsWith('/approve'))).toBe(true));
    const request = get.mock.calls[0] as unknown as [CredentialRequestOptions];
    expect(request[0].publicKey!.rpId).toBe('controller.example');
    expect([...new Uint8Array(request[0].publicKey!.challenge as ArrayBuffer)]).toEqual([1, 2, 3]);
    expect(calls.find((call) => call.path.endsWith('/approve'))?.body).toEqual({
      credential_id: encode(new Uint8Array([9])),
      authenticator_data: encode(new Uint8Array([4])),
      client_data_json: encode(new Uint8Array([5])),
      signature: encode(new Uint8Array([6])),
      approval_id: options.approval_id,
      hosts: null,
    });
  });

  it('says to add a passkey when the user has none', async () => {
    const user = userEvent.setup();
    authenticator();
    mockApi({
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: ready },
      [`POST /api/syncs/${ready.id}/approval`]: { status: 409, body: { error: 'no_passkey' } },
    });
    renderRoute(`/sync/${ready.id}`);
    await user.click(await screen.findByRole('button', { name: 'Sign and sync' }));
    expect(await screen.findByText('You have no passkey')).toBeInTheDocument();
  });

  it('says a waiting host applies at its next check-in, and only while one waits', async () => {
    const approvals = [{ id: 'a', by: 'admin', passkey: 'Laptop', created_at: plan.reported_at, host_count: 1 }];
    const checkIn = /picks up the change on its next check-in/;
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: { ...ready, status: 'applying', approvals, hosts: [picked(host.id, host.name, { step: 'waiting' })] },
      },
    });
    const { unmount } = renderRoute(`/sync/${ready.id}`);
    await openHosts();
    expect(await screen.findByText(checkIn)).toBeInTheDocument();
    unmount();
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: { ...ready, status: 'done', approvals, hosts: [picked(host.id, host.name, { step: 'applied' })] },
      },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    expect(rows()).toEqual(['build-1Applied']);
    expect(screen.queryByText(checkIn)).not.toBeInTheDocument();
  });

  it('shows a failed host with its log and retries only that host', async () => {
    const user = userEvent.setup();
    authenticator();
    const result = { plan_hash: 'p1', reported_at: plan.reported_at, received_at: null };
    const done: Sync = {
      ...ready,
      status: 'done',
      hosts: [
        picked(host.id, 'build-1', { step: 'applied', result: { ...result, log: 'done' } }),
        picked('22222222-2222-4222-8222-222222222222', 'build-2', {
          step: 'failed',
          result: { ...result, log: 'error: target not found: jq' },
        }),
        picked('33333333-3333-4333-8333-333333333333', 'build-3', { step: 'skipped', status: 'disconnected' }),
      ],
      approvals: [{ id: 'a', by: 'admin', passkey: 'Laptop', created_at: plan.reported_at, host_count: 2 }],
    };
    const calls = mockApi({
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: done },
      [`POST /api/syncs/${ready.id}/approval`]: {
        status: 200,
        body: { ...options, hosts: [{ ...options.hosts[0]!, host_id: '22222222-2222-4222-8222-222222222222' }] },
      },
      [`POST /api/syncs/${ready.id}/approve`]: { status: 200, body: done },
    });
    renderRoute(`/sync/${ready.id}`);
    expect(await screen.findByText(/Sync finished with failures/)).toBeInTheDocument();
    expect(screen.getByText('1 host applied, 1 failed, 1 skipped.', { exact: false })).toBeInTheDocument();
    await openHosts();
    expect(screen.getByText('error: target not found: jq')).toBeInTheDocument();
    const install = screen.getByRole('table', { name: 'Install (1)' });
    expect(within(install).getByText('jq')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Retry build-2' }));
    await waitFor(() =>
      expect(calls.find((call) => call.path.endsWith('/approval'))?.body).toEqual({
        hosts: ['22222222-2222-4222-8222-222222222222'],
      }),
    );
    await waitFor(() =>
      expect(calls.find((call) => call.path.endsWith('/approve'))?.body).toMatchObject({
        hosts: ['22222222-2222-4222-8222-222222222222'],
      }),
    );
  });

  it('sums up the picked hosts in one line and opens their names', async () => {
    const user = userEvent.setup();
    const hosts = [
      picked('a', 'build-10'),
      picked('b', 'build-2', { status: 'disconnected' }),
      picked('c', 'mac-1', { os: 'macos' }),
    ];
    mockApi({ [`GET /api/syncs/${ready.id}`]: { status: 200, body: { ...ready, hosts } } });
    renderRoute(`/sync/${ready.id}`);
    expect(await screen.findByText('3 hosts: 2 Linux, 1 macOS, 1 disconnected.')).toBeInTheDocument();
    expect(screen.queryByText('build-2, build-10, mac-1.')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Show all' }));
    expect(screen.getByText('build-2, build-10, mac-1.')).toBeInTheDocument();
  });

  it('shows the plans by package and filters them by host', async () => {
    const user = userEvent.setup();
    const htop = { id: { kind: 'package', name: 'htop' }, current: null, desired: 'latest', action: 'create' };
    const names = ['build-1', 'build-2', 'build-3', 'build-4', 'build-5'];
    const hosts = names.map((name, n) =>
      picked(`${n}`, name, n === 4 ? { plan: { ...plan, plan: { resources: [htop] } } } : {}),
    );
    hosts.push(picked('off', 'build-6', { plan: null, status: 'disconnected' }));
    mockApi({ [`GET /api/syncs/${ready.id}`]: { status: 200, body: { ...ready, hosts } } });
    renderRoute(`/sync/${ready.id}`);

    const install = await screen.findByRole('table', { name: 'Install (2)' });
    expect(within(install).getByRole('row', { name: /htop/ })).toHaveTextContent('build-5');
    await user.click(within(install).getByRole('button', { name: /show all 4 hosts for jq/ }));
    expect(await screen.findByRole('dialog')).toHaveTextContent('build-4');
    await user.keyboard('{Escape}');
    expect(screen.getByText(/No plan to sign: Offline: build-6/)).toBeInTheDocument();

    await user.click(screen.getByRole('combobox', { name: 'Show the plan of' }));
    await user.click(await screen.findByRole('option', { name: 'Only build-5' }));
    const only = await screen.findByRole('table', { name: 'Install (1)' });
    expect(within(only).queryByText('jq')).not.toBeInTheDocument();
  });

  it('says when there is nothing to sync', async () => {
    const inSync = { ...plan, exit_code: 0, plan: { resources: [] } };
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: { ...ready, status: 'done', hosts: [picked(host.id, host.name, { plan: inSync })] },
      },
    });
    renderRoute(`/sync/${ready.id}`);
    expect(await screen.findByText('Nothing to sync')).toBeInTheDocument();
  });

  it('shows the failed and offline plans when none can be signed', async () => {
    const failed = { ...plan, exit_code: 1, plan: null, error: 'mise: cannot read the config' };
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: {
          ...ready,
          status: 'done',
          hosts: [
            picked(host.id, 'build-1', { plan: failed }),
            picked('22222222-2222-4222-8222-222222222222', 'build-2', { plan: null, status: 'disconnected' }),
          ],
        },
      },
    });
    renderRoute(`/sync/${ready.id}`);
    expect(await screen.findByText('Nothing can be signed')).toBeInTheDocument();
    expect(screen.queryByText('Nothing to sync')).not.toBeInTheDocument();
    expect(screen.getByText(/No host has a plan to sign: 1 plan failed, 1 host offline/)).toBeInTheDocument();
    expect(screen.getByText(/No plan to sign: Offline: build-2\. Plan failed: build-1/)).toBeInTheDocument();
    expect(screen.getByText('mise: cannot read the config')).toBeInTheDocument();
  });

  it('shows the changes and the error of a partial plan', async () => {
    const partial = { ...plan, error: 'mise: tool not found: node@99' };
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: { ...ready, hosts: [picked(host.id, host.name, { plan: partial })] },
      },
    });
    renderRoute(`/sync/${ready.id}`);
    const install = await screen.findByRole('table', { name: 'Install (1)' });
    expect(within(install).getByText('jq')).toBeInTheDocument();
    expect(screen.getByText('mise: tool not found: node@99')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign and sync' })).toBeInTheDocument();
  });

  it('refuses to sign when the plans changed since the page loaded', async () => {
    const user = userEvent.setup();
    const { get } = authenticator();
    const calls = mockApi({
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: ready },
      [`POST /api/syncs/${ready.id}/approval`]: {
        status: 200,
        body: { ...options, hosts: [{ ...options.hosts[0]!, plan_hash: 'p2' }] },
      },
    });
    renderRoute(`/sync/${ready.id}`);
    await user.click(await screen.findByRole('button', { name: 'Sign and sync' }));
    expect(await screen.findByText('The plans changed')).toBeInTheDocument();
    expect(get).not.toHaveBeenCalled();
    expect(calls.some((call) => call.path.endsWith('/approve'))).toBe(false);
  });
});

describe('sync run', () => {
  const result = { plan_hash: 'p1', log: null, reported_at: '2026-01-02T10:05:00Z', received_at: null };
  const approvals = [{ id: 'a', by: 'admin', passkey: 'Laptop', created_at: '2026-01-02T10:02:00Z', host_count: 4 }];
  const run = (hosts: SyncHost[], status: Sync['status'] = 'done'): Sync => ({ ...ready, status, approvals, hosts });
  const feed = () =>
    within(screen.getByRole('list', { name: 'Activity' }))
      .getAllByRole('listitem')
      .map((item) => `${item.querySelector('strong')?.textContent} ${item.querySelector('.activity-text')?.textContent}`);
  const status = () => screen.getByRole('button', { name: /^Status/ });

  it('orders failed, replan, applying, waiting, applied, in sync, then skipped, by name', () => {
    const hosts = [
      picked('1', 'skipped-1', { step: 'skipped' }),
      picked('2', 'in-sync-1', { step: 'in_sync' }),
      picked('3', 'applied-1', { step: 'applied' }),
      picked('4', 'waiting-1', { step: 'waiting' }),
      picked('5', 'replan-1', { step: 'replan' }),
      picked('8', 'applying-1', { step: 'applying' }),
      picked('6', 'build-10', { step: 'failed' }),
      picked('7', 'build-2', { step: 'failed' }),
    ];
    expect(orderHosts(hosts).map((host) => host.name)).toEqual([
      'build-2',
      'build-10',
      'replan-1',
      'applying-1',
      'waiting-1',
      'applied-1',
      'in-sync-1',
      'skipped-1',
    ]);
  });

  it('lists the signing and the finished hosts, newest first', () => {
    const hosts = [
      picked('1', 'build-1', { step: 'applied', result: { ...result, reported_at: '2026-01-02T10:03:00Z' } }),
      picked('2', 'build-2', { step: 'failed', result: { ...result, reported_at: '2026-01-02T10:05:00Z' } }),
      picked('3', 'build-3', { step: 'waiting', result: null }),
    ];
    expect(activity(run(hosts)).map((event) => `${event.who} ${event.what}`)).toEqual([
      'build-2 failed',
      'build-1 applied',
      'admin signed 4 plans with passkey Laptop',
    ]);
  });

  it('lists when a host started and each step it reported', () => {
    const progress = {
      step: 'mise',
      started_at: '2026-01-02T10:03:00Z',
      updated_at: '2026-01-02T10:04:30Z',
      steps: [
        { step: 'native', at: '2026-01-02T10:03:00Z' },
        { step: 'mise', at: '2026-01-02T10:04:00Z' },
      ],
    };
    const hosts = [
      picked('1', 'build-1', { step: 'applied', progress, result: { ...result, reported_at: '2026-01-02T10:06:00Z' } }),
    ];
    expect(activity(run(hosts)).map((event) => `${event.who} ${event.what}`)).toEqual([
      'build-1 applied',
      'build-1 mise',
      'build-1 native packages',
      'build-1 started',
      'admin signed 4 plans with passkey Laptop',
    ]);
  });

  const three = [
    picked('a', 'build-a', { step: 'failed', result: { ...result, reported_at: '2026-01-02T10:05:00Z' } }),
    picked('b', 'build-b', { step: 'applied', result: { ...result, reported_at: '2026-01-02T10:04:00Z' } }),
    picked('c', 'build-c', { step: 'waiting', result: null }),
  ];

  it('filters the activity and the hosts by status, then clears the filters', async () => {
    const user = userEvent.setup();
    mockApi({ [`GET /api/syncs/${ready.id}`]: { status: 200, body: run(three, 'applying') } });
    renderRoute(`/sync/${ready.id}`);

    await screen.findByRole('list', { name: 'Activity' });
    expect(feed()).toEqual(['build-a failed', 'build-b applied', 'admin signed 4 plans with passkey Laptop']);
    const clear = screen.getByRole('button', { name: 'Clear filters' });
    expect(clear).toBeDisabled();
    await user.click(status());
    await user.click(screen.getByRole('checkbox', { name: /^Failed/ }));
    await user.click(screen.getByRole('checkbox', { name: /^Waiting/ }));
    expect(screen.getByRole('checkbox', { name: /^Waiting/ })).toBeVisible();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(status()).toHaveAccessibleName('Status: Failed, Waiting');
    expect(feed()).toEqual(['build-a failed']);
    expect(screen.getByRole('tab', { name: /^Hosts/ })).toHaveTextContent(/^Hosts\s*2$/);

    await openHosts();
    expect(rows()).toEqual(['build-aFailed', 'build-cWaiting']);
    await user.click(screen.getByRole('button', { name: 'Clear filters' }));
    expect(rows()).toEqual(['build-aFailed', 'build-cWaiting', 'build-bApplied']);
    expect(status()).toHaveAccessibleName('Status');
    expect(screen.getByRole('button', { name: 'Clear filters' })).toBeDisabled();
  });

  it('shows the newest event first, or the oldest first when asked', async () => {
    const user = userEvent.setup();
    mockApi({ [`GET /api/syncs/${ready.id}`]: { status: 200, body: run(three, 'applying') } });
    renderRoute(`/sync/${ready.id}`);

    await screen.findByRole('list', { name: 'Activity' });
    expect(feed()[0]).toBe('build-a failed');
    await user.click(screen.getByRole('radio', { name: 'Oldest first' }));
    expect(feed()).toEqual(['admin signed 4 plans with passkey Laptop', 'build-b applied', 'build-a failed']);
  });

  it('counts new events while the user reads further down, and jumps back to them', async () => {
    const user = userEvent.setup();
    let hosts = three;
    mockApi({ [`GET /api/syncs/${ready.id}`]: () => ({ status: 200, body: run(hosts, 'applying') }) });
    renderRoute(`/sync/${ready.id}`);

    await screen.findByRole('list', { name: 'Activity' });
    const scroller = document.querySelector<HTMLElement>('.run-feed')!;
    scroller.scrollTop = 120;
    fireEvent.scroll(scroller);
    hosts = [...three.slice(0, 2), picked('c', 'build-c', { step: 'applied', result: { ...result, reported_at: '2026-01-02T10:06:00Z' } })];
    focusManager.setFocused(false);
    focusManager.setFocused(true);
    await user.click(await screen.findByRole('button', { name: '1 new' }));
    expect(scroller.scrollTop).toBe(0);
    expect(screen.queryByRole('button', { name: '1 new' })).not.toBeInTheDocument();
    expect(feed()[0]).toBe('build-c applied');
    focusManager.setFocused(undefined);
  });

  it('opens a host from the activity and goes back', async () => {
    const user = userEvent.setup();
    mockApi({ [`GET /api/syncs/${ready.id}`]: { status: 200, body: run(three, 'applying') } });
    renderRoute(`/sync/${ready.id}`);

    await user.click(await screen.findByRole('button', { name: /build-b.*applied/ }));
    expect(screen.getByRole('tab', { name: /^Hosts/ })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('region', { name: 'build-b detail' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Back to activity' })).toHaveFocus();
    await user.keyboard('{Enter}');
    expect(screen.getByRole('tab', { name: /^Activity/ })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('button', { name: /build-b.*applied/ })).toHaveFocus();
  });

  it('dates a delayed report by when the host reported it', () => {
    const hosts = [
      picked('1', 'build-1', { step: 'applied', result: { ...result, reported_at: '2026-01-02T10:03:00Z', received_at: '2026-01-02T12:00:00Z' } }),
    ];
    expect(activity(run(hosts))[0]?.at).toBe('2026-01-02T10:03:00Z');
  });

  it('shows the plan of the next host after another host was picked in the filter', async () => {
    const user = userEvent.setup();
    const hosts = [
      picked('a', 'build-a', { step: 'failed', result }),
      picked('b', 'build-b', { step: 'failed', result }),
    ];
    mockApi({ [`GET /api/syncs/${ready.id}`]: { status: 200, body: run(hosts, 'done') } });
    renderRoute(`/sync/${ready.id}`);

    await openHosts();
    await user.type(screen.getByRole('searchbox', { name: 'Filter packages' }), 'zzz');
    await user.click(screen.getByRole('button', { name: /build-b/ }));
    expect(screen.getByRole('searchbox', { name: 'Filter packages' })).toHaveValue('');
  });

  it('puts the failed hosts of 40 on top, selects the first and counts each filter', async () => {
    const user = userEvent.setup();
    const hosts = Array.from({ length: 40 }, (_, n) => {
      const name = `host-${String(n).padStart(2, '0')}`;
      const step: SyncHost['step'] = [10, 20, 30].includes(n) ? 'failed' : n < 33 ? 'applied' : n < 36 ? 'waiting' : n < 39 ? 'in_sync' : 'skipped';
      return picked(`id-${n}`, name, { step, result: step === 'failed' ? { ...result, log: `boom ${name}` } : result });
    });
    mockApi({ [`GET /api/syncs/${ready.id}`]: { status: 200, body: run(hosts, 'applying') } });
    renderRoute(`/sync/${ready.id}`);

    await openHosts();
    expect(rows().slice(0, 4)).toEqual(['host-10Failed', 'host-20Failed', 'host-30Failed', 'host-33Waiting']);
    expect(screen.getByRole('region', { name: 'host-10 detail' })).toHaveTextContent('boom host-10');
    await user.click(status());
    expect(screen.getAllByRole('checkbox').map((box) => box.closest('label')?.textContent)).toEqual([
      'Failed3',
      'Applying0',
      'Waiting3',
      'Applied30',
      'In sync3',
      'Skipped1',
    ]);

    await user.click(screen.getByRole('checkbox', { name: /^Skipped/ }));
    expect(rows()).toEqual(['host-39Skipped']);
    expect(screen.getByRole('region', { name: 'host-39 detail' })).toBeInTheDocument();
    await user.click(screen.getByRole('checkbox', { name: /^Skipped/ }));
    await user.keyboard('{Escape}');
    await user.type(screen.getByRole('searchbox', { name: 'Search hosts' }), 'host-2');
    expect(rows()).toEqual(['host-20Failed', 'host-21Applied', 'host-22Applied', 'host-23Applied', 'host-24Applied', 'host-25Applied', 'host-26Applied', 'host-27Applied', 'host-28Applied', 'host-29Applied']);
  });

  it('shows a replan host with its own label and a retry', async () => {
    const user = userEvent.setup();
    authenticator();
    let retried = false;
    const calls = mockApi({
      [`GET /api/syncs/${ready.id}`]: () => ({
        status: 200,
        body: run([picked('r', 'build-r', { step: retried ? 'waiting' : 'replan', result })], retried ? 'applying' : 'done'),
      }),
      [`POST /api/syncs/${ready.id}/approval`]: { status: 200, body: { ...options, hosts: [{ ...options.hosts[0]!, host_id: 'r' }] } },
      [`POST /api/syncs/${ready.id}/approve`]: () => {
        retried = true;
        return { status: 200, body: run([picked('r', 'build-r', { step: 'waiting', result })], 'applying') };
      },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    const detail = await screen.findByRole('region', { name: 'build-r detail' });
    expect(within(detail).getByText('Plan changed, nothing ran')).toBeInTheDocument();
    expect(rows()).toEqual(['build-rPlan changed']);
    await user.click(within(detail).getByRole('button', { name: 'Retry build-r' }));
    await waitFor(() => expect(calls.find((call) => call.path.endsWith('/approve'))?.body).toMatchObject({ hosts: ['r'] }));
    expect(await screen.findByText(/picks up the change on its next check-in/)).toBeInTheDocument();
  });

  it('says why a waiting host that is offline does nothing', async () => {
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: run([picked('w', 'build-w', { step: 'waiting', status: 'disconnected', result })], 'applying'),
      },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    expect(await screen.findByText(/This host is offline, so nothing happens yet/)).toBeInTheDocument();
    expect(screen.queryByText(/picks up the change on its next check-in/)).not.toBeInTheDocument();
    expect(screen.getByRole('table', { name: 'Install (1)' })).toBeInTheDocument();
  });

  const applying = {
    step: 'mise',
    started_at: '2026-01-02T10:03:00Z',
    updated_at: '2026-01-02T10:04:30Z',
    steps: [{ step: 'mise', at: '2026-01-02T10:03:00Z' }],
  };

  it('shows an applying host with its step, the steps so far and the live log', async () => {
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: run(
          [picked('a', 'build-a', { step: 'applying', progress: applying, result: { ...result, reported_at: null, log: 'mise install node\nfetching' } })],
          'applying',
        ),
      },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    const detail = await screen.findByRole('region', { name: 'build-a detail' });
    expect(rows()).toEqual(['build-aApplying']);
    expect(within(detail).getByText(/^Applying: mise, started /)).toBeInTheDocument();
    const steps = within(detail).getByRole('list', { name: 'build-a apply steps' });
    expect(within(steps).getAllByRole('listitem').map((step) => step.textContent?.split(/\d/)[0])).toEqual([
      'Signed',
      'Checked',
      'mise',
      'Reported',
    ]);
    expect(within(steps).getByText('mise').closest('li')).toHaveAttribute('aria-current', 'step');
    expect(within(detail).getByText('fetching')).toBeInTheDocument();
    expect(screen.queryByText(/No report since/)).not.toBeInTheDocument();
  });

  it('warns when an applying host stops reporting, and keeps it applying', async () => {
    mockApi({
      [`GET /api/syncs/${ready.id}`]: {
        status: 200,
        body: run([picked('a', 'build-a', { step: 'applying', status: 'disconnected', progress: applying, result })], 'applying'),
      },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    expect(await screen.findByText(/^No report since .*its result arrives when it reports again\.$/)).toBeInTheDocument();
    expect(rows()).toEqual(['build-aApplying']);
  });

  it('marks the steps a failed host ran as done and the report as failed', async () => {
    const progress = { ...applying, step: 'native', steps: [{ step: 'native', at: '2026-01-02T10:03:00Z' }] };
    mockApi({
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: run([picked('f', 'build-f', { step: 'failed', progress, result })]) },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    const steps = await screen.findByRole('list', { name: 'build-f apply steps' });
    const state = (label: string) => within(steps).getByText(label).closest('[data-state]')?.getAttribute('data-state');
    expect(state('Native packages')).toBe('done');
    expect(state('mise')).toBe('pending');
    expect(state('Reported')).toBe('failed');
  });

  it('shows only signed and reported for an agent that sends no progress', async () => {
    mockApi({
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: run([picked('o', 'build-o', { step: 'applied', result })]) },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    const steps = await screen.findByRole('list', { name: 'build-o apply steps' });
    expect(within(steps).getAllByRole('listitem')).toHaveLength(2);
    expect(within(steps).getByText('Signed')).toBeInTheDocument();
    expect(within(steps).getByText('Reported')).toBeInTheDocument();
  });

  it('says the controller keeps only the end of a long log', async () => {
    const log = `…${'x'.repeat(50)}\nlast line`;
    mockApi({
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: run([picked('f', 'f'.repeat(80), { step: 'failed', result: { ...result, log } })]) },
    });
    renderRoute(`/sync/${ready.id}`);
    await openHosts();
    expect(await screen.findByText('The controller keeps only the end of a long log.')).toBeInTheDocument();
    expect(screen.getByText('last line')).toBeInTheDocument();
  });
});

describe('drift actions', () => {
  const state: HostState = {
    roles: [],
    adjustments: [],
    items: [],
    config: '',
    health: 'drift',
    drift: { ...plan, status: 'drift', ignored: false },
    undeclared: { manager: 'pacman', packages: ['sl'], ignored_count: 0, explicit_count: 3, reported_at: plan.reported_at },
  };

  function renderDrift(extra: Parameters<typeof mockApi>[0] = {}) {
    const calls = mockApi({
      [`GET /api/hosts/${host.id}`]: { status: 200, body: host },
      [`GET /api/hosts/${host.id}/state`]: { status: 200, body: state },
      'GET /api/roles': { status: 200, body: [] },
      ...extra,
    });
    renderRoute(`/hosts/${host.id}`);
    return calls;
  }

  it('reverts the drift with a sync of this host', async () => {
    const user = userEvent.setup();
    const calls = renderDrift({
      'POST /api/syncs': { status: 201, body: ready },
      [`GET /api/syncs/${ready.id}`]: { status: 200, body: ready },
    });
    await user.click(await screen.findByRole('tab', { name: 'Drift' }));
    await user.click(await screen.findByRole('button', { name: 'Revert' }));
    expect(await screen.findByRole('button', { name: 'Sign and sync' })).toBeInTheDocument();
    expect(calls.find((call) => call.path === '/api/syncs' && call.method === 'POST')?.body).toEqual({
      hosts: [host.id],
    });
  });

  it('adopts an undeclared package on this host', async () => {
    const user = userEvent.setup();
    const calls = renderDrift({
      [`POST /api/hosts/${host.id}/drift/adopt`]: { status: 204 },
    });
    await user.click(await screen.findByRole('tab', { name: 'Drift' }));
    await user.click(await screen.findByRole('button', { name: 'Adopt sl' }));
    const dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Adopt' }));
    await waitFor(() =>
      expect(calls.find((call) => call.path.endsWith('/drift/adopt'))?.body).toEqual({ package: 'sl', role: null }),
    );
  });

  it('ignores the drift', async () => {
    const user = userEvent.setup();
    const calls = renderDrift({ [`POST /api/hosts/${host.id}/drift/ignore`]: { status: 204 } });
    await user.click(await screen.findByRole('tab', { name: 'Drift' }));
    await user.click(await screen.findByRole('button', { name: 'Ignore' }));
    await waitFor(() => expect(calls.some((call) => call.path.endsWith('/drift/ignore'))).toBe(true));
  });
});

describe('passkeys', () => {
  const passkey: Passkey = {
    id: 'p',
    login: 'admin',
    name: 'Laptop',
    credential_id: 'Bw',
    created_at: '2026-01-02T10:00:00Z',
  };

  it('creates a passkey with the browser and saves it', async () => {
    const user = userEvent.setup();
    const { create } = authenticator();
    const calls = mockApi({
      'GET /api/passkeys': { status: 200, body: [] },
      'POST /api/passkeys/options': {
        status: 200,
        body: {
          challenge: encode(new Uint8Array([1])),
          rp: { id: 'controller.example', name: 'sewa' },
          user: { id: encode(new Uint8Array([2])), name: 'admin' },
          algorithm: -7,
          exclude_credentials: [],
        },
      },
      'POST /api/passkeys': { status: 201, body: passkey },
    });
    renderRoute('/passkeys');
    expect(await screen.findByText('No passkeys yet')).toBeInTheDocument();
    await user.click(screen.getAllByRole('button', { name: 'Add a passkey' })[0]!);
    await user.type(await screen.findByRole('textbox', { name: /Name/ }), 'Laptop');
    await user.click(screen.getByRole('button', { name: 'Create passkey' }));
    await waitFor(() => expect(calls.some((call) => call.method === 'POST' && call.path === '/api/passkeys')).toBe(true));
    const request = create.mock.calls[0] as unknown as [CredentialCreationOptions];
    expect(request[0].publicKey!.pubKeyCredParams).toEqual([{ type: 'public-key', alg: -7 }]);
    expect(request[0].publicKey!.authenticatorSelection?.userVerification).toBe('required');
    expect(calls.find((call) => call.path === '/api/passkeys' && call.method === 'POST')?.body).toEqual({
      name: 'Laptop',
      credential_id: encode(new Uint8Array([7])),
      public_key: encode(new Uint8Array([10])),
      algorithm: -7,
      authenticator_data: encode(new Uint8Array([11])),
      client_data_json: encode(new Uint8Array([8])),
    });
  });
});

describe('base64url', () => {
  it('round-trips bytes without padding', () => {
    const bytes = new Uint8Array([0xfb, 0xff, 0x00, 0x10]);
    expect(encode(bytes)).toBe('-_8AEA');
    expect([...decode(encode(bytes))]).toEqual([...bytes]);
  });
});
