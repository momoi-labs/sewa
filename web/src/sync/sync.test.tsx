import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ApprovalOptions, Host, HostState, Passkey, Sync, SyncHost } from '../api';
import { mockApi, renderRoute } from '../test-utils';
import { decode, encode } from '../webauthn';

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
  change_id: ready.id,
  hosts: [{ host_id: host.id, instance_uid: 'u', config_hash: 'c1', plan_hash: 'p1aaaaaaaaaaaaaaaa' }],
};

const realCredentials = Object.getOwnPropertyDescriptor(navigator, 'credentials');

afterEach(() => {
  vi.unstubAllGlobals();
  if (realCredentials) Object.defineProperty(navigator, 'credentials', realCredentials);
  else Reflect.deleteProperty(navigator, 'credentials');
});

/** A browser with WebAuthn whose authenticator answers `get` and `create`. */
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
        body: [host, { ...host, id: '22222222-2222-4222-8222-222222222222', name: 'build-2', os: 'macos' }],
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

  it('lists the hosts to pick in natural order', async () => {
    const named = (name: string) => ({ ...host, id: `id-${name}`, name });
    mockApi({
      'GET /api/hosts': { status: 200, body: [named('build-10'), named('build-2'), named('build-1')] },
      'GET /api/syncs': { status: 200, body: [] },
      'GET /api/syncs/edits': { status: 200, body: { since: null, edits: [] } },
    });
    renderRoute('/sync');
    const list = await screen.findByRole('list', { name: 'Hosts to sync' });
    const names = within(list).getAllByRole('listitem').map((item) => item.querySelector('.host-picker-name')?.textContent);
    expect(names).toEqual(['build-1', 'build-2', 'build-10']);
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
      [`POST /api/syncs/${ready.id}/approval`]: { status: 200, body: options },
      [`POST /api/syncs/${ready.id}/approve`]: { status: 200, body: done },
    });
    renderRoute(`/sync/${ready.id}`);
    expect(await screen.findByText(/Sync finished with failures/)).toBeInTheDocument();
    expect(screen.getByText('1 host applied, 1 failed, 1 skipped.', { exact: false })).toBeInTheDocument();
    expect(screen.getByText('error: target not found: jq')).toBeInTheDocument();
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
