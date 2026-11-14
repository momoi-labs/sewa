import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ApprovalOptions, ChangeDetail, Host, HostState, Passkey } from '../api';
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

function affected(id: string, name: string, result: ChangeDetail['hosts'][number]['result']) {
  return {
    id,
    name,
    status: 'connected' as const,
    current_config: '',
    proposed_config: null,
    plan,
    result,
  };
}

const draft: ChangeDetail = {
  id: '44444444-4444-4444-8444-444444444444',
  status: 'draft',
  created_at: '2026-01-02T10:00:00Z',
  created_by: 'admin',
  closed_at: null,
  edit_count: 1,
  revert_host: null,
  edits: [],
  hosts: [affected(host.id, host.name, null)],
  approvals: [],
};

const options: ApprovalOptions = {
  challenge: encode(new Uint8Array([1, 2, 3])),
  rp_id: 'controller.example',
  allow_credentials: [encode(new Uint8Array([9]))],
  signed_text: 'sewa approval v1\n',
  change_id: draft.id,
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

describe('approve', () => {
  it('signs the plans with a passkey and sends the assertion', async () => {
    const user = userEvent.setup();
    const { get } = authenticator();
    const calls = mockApi({
      [`GET /api/changes/${draft.id}`]: { status: 200, body: draft },
      [`POST /api/changes/${draft.id}/approval`]: { status: 200, body: options },
      [`POST /api/changes/${draft.id}/approve`]: { status: 200, body: { ...draft, status: 'approved' } },
    });
    renderRoute(`/changes/${draft.id}`);
    await user.click(await screen.findByRole('button', { name: 'Approve' }));
    const dialog = await screen.findByRole('dialog');
    const row = (await within(dialog).findByText('build-1')).closest('tr')!;
    expect(within(row).getByText('p1aaaaaaaaaa')).toBeInTheDocument();
    expect(calls.some((call) => call.path.endsWith('/approve'))).toBe(false);
    await user.click(within(dialog).getByRole('button', { name: 'Approve with passkey' }));
    await waitFor(() => expect(calls.some((call) => call.path.endsWith('/approve'))).toBe(true));
    const request = get.mock.calls[0] as unknown as [CredentialRequestOptions];
    expect(request[0].publicKey!.rpId).toBe('controller.example');
    expect([...new Uint8Array(request[0].publicKey!.challenge as ArrayBuffer)]).toEqual([1, 2, 3]);
    expect(calls.find((call) => call.path.endsWith('/approve'))?.body).toEqual({
      credential_id: encode(new Uint8Array([9])),
      authenticator_data: encode(new Uint8Array([4])),
      client_data_json: encode(new Uint8Array([5])),
      signature: encode(new Uint8Array([6])),
    });
  });

  it('says to add a passkey when the user has none', async () => {
    const user = userEvent.setup();
    authenticator();
    mockApi({
      [`GET /api/changes/${draft.id}`]: { status: 200, body: draft },
      [`POST /api/changes/${draft.id}/approval`]: { status: 409, body: { error: 'no_passkey' } },
    });
    renderRoute(`/changes/${draft.id}`);
    await user.click(await screen.findByRole('button', { name: 'Approve' }));
    expect(await screen.findByText('You have no passkey')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Approve with passkey' })).toBeDisabled();
  });

  it('shows each host result and a summary', async () => {
    const approved: ChangeDetail = {
      ...draft,
      status: 'approved',
      hosts: [
        affected(host.id, 'build-1', {
          status: 'applied',
          plan_hash: 'p1',
          log: 'done',
          reported_at: plan.reported_at,
          received_at: '2026-01-02T11:30:00Z',
        }),
        affected('22222222-2222-4222-8222-222222222222', 'build-2', {
          status: 'failed',
          plan_hash: 'p1',
          log: 'error: target not found: jq',
          reported_at: plan.reported_at,
          received_at: null,
        }),
        affected('33333333-3333-4333-8333-333333333333', 'build-3', {
          status: 'replan',
          plan_hash: 'p1',
          log: 'the plan changed',
          reported_at: plan.reported_at,
          received_at: null,
        }),
        affected('55555555-5555-4555-8555-555555555555', 'build-4', {
          status: 'waiting',
          plan_hash: 'p1',
          log: null,
          reported_at: null,
          received_at: null,
        }),
      ],
      approvals: [{ id: 'a', by: 'admin', passkey: 'Laptop', created_at: plan.reported_at, host_count: 4 }],
    };
    mockApi({ [`GET /api/changes/${draft.id}`]: { status: 200, body: approved } });
    renderRoute(`/changes/${draft.id}`);
    expect(
      await screen.findByText('Applied on 1 of 4 hosts, 1 failed, 1 need re-approval, 1 waiting.'),
    ).toBeInTheDocument();
    expect(screen.getByText('Applied')).toBeInTheDocument();
    // The agent kept this result while it could not reach the controller.
    expect(screen.getByText(/applied the plan.*kept by the agent while offline/)).toBeInTheDocument();
    expect(screen.getByText('Failed')).toBeInTheDocument();
    expect(screen.getByText('Needs re-approval')).toBeInTheDocument();
    expect(screen.getByText('error: target not found: jq')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Approve again' })).toBeInTheDocument();
  });
});

describe('drift actions', () => {
  const state: HostState = {
    roles: [],
    proposed_roles: null,
    adjustments: [],
    proposed_adjustments: null,
    items: [],
    config: '',
    health: 'drift',
    drift: { ...plan, status: 'drift', ignored: false, revert_change: null },
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

  it('reverts the drift by opening a change', async () => {
    const user = userEvent.setup();
    const calls = renderDrift({
      [`POST /api/hosts/${host.id}/drift/revert`]: { status: 201, body: { ...draft, revert_host: host } },
      [`GET /api/changes/${draft.id}`]: { status: 200, body: { ...draft, revert_host: { id: host.id, name: host.name } } },
    });
    await user.click(await screen.findByRole('tab', { name: 'Drift' }));
    await user.click(await screen.findByRole('button', { name: 'Revert' }));
    expect(await screen.findByRole('heading', { name: 'Revert of build-1' })).toBeInTheDocument();
    expect(calls.some((call) => call.path === `/api/hosts/${host.id}/drift/revert`)).toBe(true);
  });

  it('adopts an undeclared package on this host', async () => {
    const user = userEvent.setup();
    const calls = renderDrift({
      [`POST /api/hosts/${host.id}/drift/adopt`]: { status: 201, body: draft },
    });
    await user.click(await screen.findByRole('tab', { name: 'Drift' }));
    await user.click(await screen.findByRole('button', { name: 'Adopt sl' }));
    const dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Add to draft' }));
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
