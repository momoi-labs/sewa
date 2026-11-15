import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { retryQuery, type Host } from '../api';
import { formatDateTime, formatReceived } from '../format';
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
  it('pulses the dot of a disconnected host only', async () => {
    mockApi({
      'GET /api/hosts': {
        status: 200,
        body: [connected, { ...connected, id: 'x2', name: 'build-3', status: 'disconnected' }, waiting],
      },
    });
    renderRoute('/');
    const dot = async (name: string) =>
      (await screen.findByRole('link', { name })).closest('tr')!.querySelector('[data-slot="dot"]')!;
    expect(await dot('build-3')).toHaveClass('dot-pulse');
    expect(await dot('build-1')).not.toHaveClass('dot-pulse');
    expect(await dot('laptop')).not.toHaveClass('dot-pulse');
  });

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

  it('shows the 403 page to a person who is not an admin', async () => {
    mockApi({ 'GET /api/hosts': { status: 403, body: { error: 'not_admin' } } });
    renderRoute('/');
    expect(await screen.findByRole('heading', { name: 'Access denied' })).toBeInTheDocument();
    expect(screen.getByText('Error 403')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Check again' })).toBeInTheDocument();
  });

  it('shows the error page without the shell', async () => {
    mockApi({ 'GET /api/hosts': { status: 403, body: { error: 'not_admin' } } });
    renderRoute('/');
    await screen.findByRole('heading', { name: 'Access denied' });
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument();
    expect(screen.queryByRole('banner')).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /sewa/ })).not.toBeInTheDocument();
    expect(document.querySelector('.error-page-screen')).not.toBeNull();
  });

  it('does not retry a 403 and keeps the page, but retries a 500', async () => {
    const forbidden = mockApi({ 'GET /api/hosts': { status: 403, body: { error: 'not_admin' } } });
    const first = renderRoute('/', { retry: retryQuery, retryDelay: 1, refetchInterval: 20 });
    await screen.findByRole('heading', { name: 'Access denied' });
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(screen.getByRole('heading', { name: 'Access denied' })).toBeInTheDocument();
    expect(forbidden.filter((c) => c.path === '/api/hosts')).toHaveLength(1);
    first.unmount();

    const broken = mockApi({ 'GET /api/hosts': { status: 500, body: { error: 'internal' } } });
    renderRoute('/', { retry: retryQuery, retryDelay: 1 });
    await screen.findByRole('heading', { name: 'Something broke on our side' });
    expect(broken.filter((c) => c.path === '/api/hosts')).toHaveLength(4);
  });

  it('shows the 500 page when the controller fails, and retries', async () => {
    const user = userEvent.setup();
    const calls = mockApi({ 'GET /api/hosts': { status: 500, body: { error: 'internal' } } });
    renderRoute('/');
    expect(await screen.findByRole('heading', { name: 'Something broke on our side' })).toBeInTheDocument();
    expect(screen.getByText('Error 500')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Retry' }));
    expect(calls.filter((c) => c.path === '/api/hosts').length).toBeGreaterThan(1);
  });

  it('shows the no-answer page when the controller does not answer', async () => {
    mockApi({});
    globalThis.fetch = (() => Promise.reject(new TypeError('Failed to fetch'))) as typeof fetch;
    renderRoute('/');
    expect(await screen.findByRole('heading', { name: 'The controller did not answer' })).toBeInTheDocument();
    expect(screen.getByText('No answer')).toBeInTheDocument();
    const art = document.querySelector('img.error-page-art');
    expect(art).toHaveAttribute('data-art', 'surprised');
    expect(art?.getAttribute('src')).toMatch(/^data:image\/svg\+xml/);
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
          {
            id: 5,
            kind: 'drift.detected',
            at: '2026-01-01T11:30:00Z',
            data: { received_at: '2026-01-01T12:00:00Z' },
          },
          {
            id: 4,
            kind: 'host.offline',
            at: '2026-01-01T11:00:00Z',
            data: { from: '2026-01-01T11:00:00Z', until: '2026-01-01T12:00:00Z' },
          },
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
      `The host drifted from its desired state${formatReceived('2026-01-01T12:00:00Z')}`,
      `No reports from ${formatDateTime('2026-01-01T11:00:00Z')} to ${formatDateTime('2026-01-01T12:00:00Z')}`,
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
