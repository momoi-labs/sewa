import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import type { Host } from '../api';
import { mockApi, renderRoute } from '../test-utils';

const seen = new Date(Date.now() - 12_000).toISOString();

const connected: Host = {
  id: '11111111-1111-4111-8111-111111111111',
  name: 'build-1',
  os: 'linux',
  status: 'connected',
  created_at: '2026-01-01T10:00:00Z',
  enrolled_at: '2026-01-01T10:01:00Z',
  last_seen_at: seen,
  agent: { version: '0.1.0', hostname: 'build-1', os: 'linux', arch: 'amd64', user: 'root' },
};

const waiting: Host = {
  id: '22222222-2222-4222-8222-222222222222',
  name: 'laptop',
  os: 'macos',
  status: 'waiting',
  created_at: '2026-01-01T11:00:00Z',
  enrolled_at: null,
  last_seen_at: null,
  agent: null,
};

describe('hosts', () => {
  it('lists each host with its status, agent version and last report', async () => {
    mockApi({ 'GET /api/hosts': { status: 200, body: [connected, waiting] } });
    renderRoute('/');
    const row = (await screen.findByRole('link', { name: 'build-1' })).closest('tr')!;
    expect(within(row).getByText('Linux')).toBeInTheDocument();
    expect(within(row).getByText('Connected')).toBeInTheDocument();
    expect(within(row).getByText('0.1.0')).toBeInTheDocument();
    expect(within(row).getByText(/seconds ago/)).toBeInTheDocument();
    const other = screen.getByRole('link', { name: 'laptop' }).closest('tr')!;
    expect(within(other).getByText('macOS')).toBeInTheDocument();
    expect(within(other).getByText('Waiting')).toBeInTheDocument();
    expect(within(other).getByRole('img', { name: 'The agent has not reported yet' })).toBeInTheDocument();
    expect(screen.getByText('2 hosts')).toBeInTheDocument();
  });

  it('offers to add the first host', async () => {
    mockApi({ 'GET /api/hosts': { status: 200, body: [] } });
    renderRoute('/');
    expect(await screen.findByText('No hosts yet')).toBeInTheDocument();
  });

  it('says why the list cannot load', async () => {
    mockApi({ 'GET /api/hosts': { status: 403, body: { error: 'not_admin' } } });
    renderRoute('/');
    expect(await screen.findByText('You are not an admin')).toBeInTheDocument();
  });

  it('creates a host and shows its install command once', async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      'GET /api/hosts': { status: 200, body: [] },
      'POST /api/hosts': (body) =>
        (body as { name: string }).name === 'taken'
          ? { status: 409, body: { error: 'name_taken' } }
          : {
              status: 201,
              body: {
                host: { ...waiting, name: 'mac-1' },
                install_command: "curl -fsSL 'http://controller.example/install' | sudo sh -s -- 'secret'",
                token_expires_at: '2026-01-01T12:00:00Z',
              },
            },
    });
    renderRoute('/');
    await user.click(await screen.findByRole('button', { name: 'Add a host' }));
    const dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Add host' }));
    expect(within(dialog).getByText('Enter a name.')).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText('Name'), 'taken');
    await user.click(within(dialog).getByRole('button', { name: 'Add host' }));
    expect(await within(dialog).findByText('A host has this name. Choose another.')).toBeInTheDocument();

    await user.clear(within(dialog).getByLabelText('Name'));
    await user.type(within(dialog).getByLabelText('Name'), 'mac-1');
    await user.click(within(dialog).getByRole('radio', { name: 'macOS' }));
    await user.click(within(dialog).getByRole('button', { name: 'Add host' }));
    expect(await within(dialog).findByText('Install the agent on mac-1')).toBeInTheDocument();
    expect(within(dialog).getByText(/sudo sh -s -- 'secret'/)).toBeInTheDocument();
    expect(within(dialog).getByText(/works once and is valid for 1 hour/)).toBeInTheDocument();
    expect(within(dialog).getByRole('button', { name: /copy/i })).toBeInTheDocument();
    expect(calls.filter((call) => call.method === 'POST').at(-1)?.body).toEqual({ name: 'mac-1', os: 'macos' });
  });

  it('shows a host with its events, newest first', async () => {
    mockApi({
      [`GET /api/hosts/${connected.id}`]: { status: 200, body: connected },
      [`GET /api/hosts/${connected.id}/events`]: {
        status: 200,
        body: [
          { id: 3, kind: 'host.connected', at: '2026-01-01T10:01:05Z', data: {} },
          { id: 2, kind: 'host.enrolled', at: '2026-01-01T10:01:00Z', data: {} },
          { id: 1, kind: 'host.created', at: '2026-01-01T10:00:00Z', data: { name: 'build-1', os: 'linux' } },
        ],
      },
    });
    const user = userEvent.setup();
    renderRoute(`/hosts/${connected.id}`);
    expect(await screen.findByRole('heading', { name: 'build-1' })).toBeInTheDocument();
    expect(screen.getByText('amd64')).toBeInTheDocument();
    await user.click(screen.getByRole('tab', { name: 'Events' }));
    const rows = (await screen.findAllByRole('row')).slice(1);
    expect(rows.map((row) => within(row).getAllByRole('cell')[1]!.textContent)).toEqual([
      'Agent connected',
      'Install token traded for an agent credential',
      'Host created for Linux',
    ]);
  });

  it('says when a host does not exist', async () => {
    mockApi({});
    renderRoute('/hosts/unknown');
    expect(await screen.findByText('This host does not exist')).toBeInTheDocument();
  });
});
