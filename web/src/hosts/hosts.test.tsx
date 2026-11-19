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
  disabled_at: null,
  enrolled_at: '2026-01-01T10:01:00Z',
  last_seen_at: seen,
  agent: { version: '0.1.0', hostname: 'build-1', os: 'linux', arch: 'amd64', user: 'root' },
};

const waiting: Host = {
  id: '22222222-2222-4222-8222-222222222222',
  name: 'laptop',
  os: null,
  status: 'waiting',
  created_at: '2026-01-01T11:00:00Z',
  disabled_at: null,
  enrolled_at: null,
  last_seen_at: null,
  agent: null,
};

describe('hosts', () => {
  it('pulses no host dot', async () => {
    mockApi({
      'GET /api/hosts': {
        status: 200,
        body: [connected, { ...connected, id: 'x2', name: 'build-3', status: 'disconnected' }, waiting],
      },
    });
    renderRoute('/');
    const dot = async (name: string) =>
      (await screen.findByRole('link', { name })).closest('tr')!.querySelector('[data-slot="dot"]')!;
    expect(await dot('build-3')).not.toHaveClass('dot-pulse');
    expect(await dot('build-1')).not.toHaveClass('dot-pulse');
  });

  it('filters the hosts by name and status', async () => {
    const user = userEvent.setup();
    mockApi({ 'GET /api/hosts': { status: 200, body: [connected, waiting] } });
    renderRoute('/');
    expect(await screen.findByText('2 of 2 hosts')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Clear filters' })).not.toBeInTheDocument();
    await user.type(screen.getByRole('searchbox', { name: 'Search hosts' }), 'lap');
    expect(screen.queryByRole('link', { name: 'build-1' })).not.toBeInTheDocument();
    expect(screen.getByText('1 of 2 hosts')).toBeInTheDocument();
    await user.type(screen.getByRole('searchbox', { name: 'Search hosts' }), 'x');
    expect(screen.getByText('No hosts match your filters.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Clear filters' }));
    expect(screen.getByRole('link', { name: 'build-1' })).toBeInTheDocument();
    expect(screen.getByText('2 of 2 hosts')).toBeInTheDocument();
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
    expect(within(other).getByRole('img', { name: 'The agent reports the OS when it connects' })).toBeInTheDocument();
    expect(within(other).getByText('Waiting')).toBeInTheDocument();
    expect(within(other).getByRole('img', { name: 'The agent has not reported yet' })).toBeInTheDocument();
    expect(screen.getByText('2 of 2 hosts')).toBeInTheDocument();
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
    expect(within(dialog).queryByRole('radio')).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Add host' }));
    expect(within(dialog).getByText('Enter a name.')).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText('Name'), 'taken');
    await user.click(within(dialog).getByRole('button', { name: 'Add host' }));
    expect(await within(dialog).findByText('A host has this name. Choose another.')).toBeInTheDocument();

    await user.clear(within(dialog).getByLabelText('Name'));
    await user.type(within(dialog).getByLabelText('Name'), 'mac-1');
    await user.click(within(dialog).getByRole('button', { name: 'Add host' }));
    expect(await within(dialog).findByText('Install the agent on mac-1')).toBeInTheDocument();
    expect(within(dialog).getByText(/sudo sh -s -- 'secret'/)).toBeInTheDocument();
    expect(within(dialog).getByText(/works once and is valid for 1 hour/)).toBeInTheDocument();
    expect(within(dialog).getByRole('button', { name: /copy/i })).toBeInTheDocument();
    expect(calls.filter((call) => call.method === 'POST').at(-1)?.body).toEqual({ name: 'mac-1' });
  });

  it('shows the instance, the list and the host in the header breadcrumb', async () => {
    mockApi({
      [`GET /api/hosts/${connected.id}`]: { status: 200, body: connected },
      [`GET /api/hosts/${connected.id}/events`]: { status: 200, body: [] },
    });
    expect(window.location.port).not.toBe('');
    renderRoute(`/hosts/${connected.id}`);
    const trail = await screen.findByRole('navigation', { name: 'Breadcrumb' });
    expect(await within(trail).findByRole('link', { name: window.location.host })).toHaveAttribute('href', '/');
    expect(within(trail).getByRole('link', { name: 'Hosts' })).toHaveAttribute('href', '/');
    expect(await within(trail).findByText('build-1')).toHaveAttribute('aria-current', 'page');
    expect(screen.getAllByRole('navigation', { name: 'Breadcrumb' })).toHaveLength(1);
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

  it('disables a host and shows the disabled state', async () => {
    const user = userEvent.setup();
    let disabled = false;
    const off: Host = { ...connected, status: 'disabled', disabled_at: '2026-01-02T10:00:00Z' };
    const calls = mockApi({
      [`GET /api/hosts/${connected.id}`]: () => ({ status: 200, body: disabled ? off : connected }),
      [`GET /api/hosts/${connected.id}/events`]: { status: 200, body: [] },
      [`GET /api/hosts/${connected.id}/state`]: { status: 404, body: { error: 'not_found' } },
      [`POST /api/hosts/${connected.id}/disable`]: () => {
        disabled = true;
        return { status: 200, body: off };
      },
    });
    renderRoute(`/hosts/${connected.id}`);
    await user.click(await screen.findByRole('button', { name: 'Disable' }));
    expect(await screen.findByText('This host is disabled')).toBeInTheDocument();
    expect(calls.some((c) => c.method === 'POST' && c.path.endsWith('/disable'))).toBe(true);
    expect(screen.getByRole('button', { name: 'Enable' })).toBeInTheDocument();
  });

  it('enables a disabled host and shows its new install command', async () => {
    const user = userEvent.setup();
    const off: Host = { ...connected, status: 'disabled', disabled_at: '2026-01-02T10:00:00Z' };
    mockApi({
      [`GET /api/hosts/${connected.id}`]: { status: 200, body: off },
      [`GET /api/hosts/${connected.id}/events`]: { status: 200, body: [] },
      [`GET /api/hosts/${connected.id}/state`]: { status: 404, body: { error: 'not_found' } },
      [`POST /api/hosts/${connected.id}/enable`]: {
        status: 200,
        body: {
          host: { ...off, status: 'waiting', disabled_at: null },
          install_command: "curl -fsSL 'http://controller.example/install' | sudo sh -s -- 'fresh'",
          token_expires_at: '2026-01-02T11:00:00Z',
        },
      },
    });
    renderRoute(`/hosts/${connected.id}`);
    await user.click(await screen.findByRole('button', { name: 'Enable' }));
    expect(await screen.findByText(/sudo sh -s -- 'fresh'/)).toBeInTheDocument();
  });

  it('asks before it deletes a host, then returns to the list', async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      [`GET /api/hosts/${connected.id}`]: { status: 200, body: connected },
      [`GET /api/hosts/${connected.id}/events`]: { status: 200, body: [] },
      [`GET /api/hosts/${connected.id}/state`]: { status: 404, body: { error: 'not_found' } },
      'GET /api/hosts': { status: 200, body: [] },
      [`DELETE /api/hosts/${connected.id}`]: { status: 204 },
    });
    renderRoute(`/hosts/${connected.id}`);
    await user.click(await screen.findByRole('button', { name: 'Delete host' }));
    const dialog = await screen.findByRole('alertdialog', { name: 'Delete build-1?' });
    expect(calls.some((c) => c.method === 'DELETE')).toBe(false);
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(calls.some((c) => c.method === 'DELETE')).toBe(false);
    await user.click(screen.getByRole('button', { name: 'Delete host' }));
    await user.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: 'Delete host' }));
    expect(await screen.findByText('No hosts yet')).toBeInTheDocument();
    expect(calls.filter((c) => c.method === 'DELETE')).toHaveLength(1);
  });
});
