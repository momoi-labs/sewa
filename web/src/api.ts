import { queryOptions } from '@tanstack/react-query';

/** How often the screens read the API again, so they follow the agents. */
export const POLL_MS = 5000;

export type HostStatus = 'waiting' | 'connected' | 'disconnected';
export type Os = 'linux' | 'macos';

export type Agent = {
  version: string | null;
  hostname: string | null;
  os: string | null;
  arch: string | null;
  user: string | null;
};

export type Host = {
  id: string;
  name: string;
  os: Os;
  status: HostStatus;
  created_at: string;
  enrolled_at: string | null;
  last_seen_at: string | null;
  agent: Agent | null;
};

export type Created = {
  host: Host;
  install_command: string;
  token_expires_at: string;
};

export type HostEvent = {
  id: number;
  kind: string;
  at: string;
  data: Record<string, unknown>;
};

/** A failed API call. `code` is the server's error code, or `network`. */
export class ApiFailure extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string) {
    super(`API call failed: ${status} ${code}`);
    this.status = status;
    this.code = code;
  }
}

/** Calls the controller's API with JSON in and out. Throws ApiFailure on any error. */
export async function api<T>(method: 'GET' | 'POST', path: string, body?: unknown): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, {
      method,
      credentials: 'same-origin',
      headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiFailure(0, 'network');
  }
  const data: unknown = await response.json().catch(() => null);
  if (!response.ok) {
    const code = (data as { error?: unknown } | null)?.error;
    throw new ApiFailure(response.status, typeof code === 'string' ? code : 'internal');
  }
  return data as T;
}

export const hostsQuery = queryOptions({
  queryKey: ['hosts'],
  queryFn: () => api<Host[]>('GET', '/api/hosts'),
  refetchInterval: POLL_MS,
});

export const hostQuery = (id: string) =>
  queryOptions({
    queryKey: ['hosts', id],
    queryFn: () => api<Host>('GET', `/api/hosts/${encodeURIComponent(id)}`),
    refetchInterval: POLL_MS,
  });

export const eventsQuery = (id: string) =>
  queryOptions({
    queryKey: ['hosts', id, 'events'],
    queryFn: () => api<HostEvent[]>('GET', `/api/hosts/${encodeURIComponent(id)}/events`),
    refetchInterval: POLL_MS,
  });

export function createHost(name: string, os: Os) {
  return api<Created>('POST', '/api/hosts', { name, os });
}
