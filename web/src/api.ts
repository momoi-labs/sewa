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

export type ItemKind = 'package' | 'tool';

/** A system package (a mise bootstrap key) or a mise tool, and its version. */
export type Item = { kind: ItemKind; name: string; version: string };

export type Named = { id: string; name: string };

export type Role = {
  id: string;
  name: string;
  created_at: string;
  items: Item[];
  /** The items with the draft change applied, or null without a draft. */
  proposed_items: Item[] | null;
  hosts: Named[];
  proposed_hosts: Named[] | null;
};

export type ChangeStatus = 'draft' | 'approved' | 'discarded';

export type Change = {
  id: string;
  status: ChangeStatus;
  created_at: string;
  created_by: string;
  closed_at: string | null;
  edit_count: number;
};

export type Edit =
  | { op: 'role_item_set'; role: string; kind: ItemKind; name: string; version: string }
  | { op: 'role_item_remove'; role: string; kind: ItemKind; name: string }
  | { op: 'host_role_add'; host: string; role: string }
  | { op: 'host_role_remove'; host: string; role: string }
  | { op: 'host_item_set'; host: string; kind: ItemKind; name: string; version: string }
  | { op: 'host_item_remove'; host: string; kind: ItemKind; name: string }
  | { op: 'host_item_reset'; host: string; kind: ItemKind; name: string };

export type EditEntry = { id: number; at: string; by: string; edit: Edit };

/** One resource of a mise bootstrap plan. */
export type PlanResource = {
  id: { kind: string; name: string };
  current?: string | null;
  desired?: string | null;
  action: 'noop' | 'create' | 'update' | 'remove' | 'unknown' | string;
};

/** What the agent found when it planned a config. */
export type Plan = {
  config_hash: string;
  plan_hash: string;
  /** 0 in sync, 2 changes, 1 an error or an unknown state. */
  exit_code: number;
  plan: { resources?: PlanResource[]; summary?: Record<string, number> } | null;
  error: string | null;
  reported_at: string;
  /** The plan is of an older config; a new one is on its way. */
  stale: boolean;
};

export type AffectedHost = {
  id: string;
  name: string;
  status: HostStatus;
  current_config: string;
  proposed_config: string | null;
  plan: Plan | null;
};

export type ChangeDetail = Change & { edits: EditEntry[]; hosts: AffectedHost[] };

export type Adjustment = { kind: ItemKind; name: string } & (
  | { action: 'set'; version: string }
  | { action: 'remove' }
);

export type HostState = {
  roles: Named[];
  proposed_roles: Named[] | null;
  adjustments: Adjustment[];
  proposed_adjustments: Adjustment[] | null;
  items: Item[];
  config: string;
  health: string | null;
  drift: (Plan & { status: 'in_sync' | 'drift' | 'error' }) | null;
  undeclared: { manager: string; packages: string[]; explicit_count: number; reported_at: string } | null;
};

export const hostStateQuery = (id: string) =>
  queryOptions({
    queryKey: ['hosts', id, 'state'],
    queryFn: () => api<HostState>('GET', `/api/hosts/${encodeURIComponent(id)}/state`),
    refetchInterval: POLL_MS,
  });

export const rolesQuery = queryOptions({
  queryKey: ['roles'],
  queryFn: () => api<Role[]>('GET', '/api/roles'),
  refetchInterval: POLL_MS,
});

export const roleQuery = (id: string) =>
  queryOptions({
    queryKey: ['roles', id],
    queryFn: () => api<Role>('GET', `/api/roles/${encodeURIComponent(id)}`),
    refetchInterval: POLL_MS,
  });

export const changesQuery = queryOptions({
  queryKey: ['changes'],
  queryFn: () => api<Change[]>('GET', '/api/changes'),
  refetchInterval: POLL_MS,
});

export const changeQuery = (id: string) =>
  queryOptions({
    queryKey: ['changes', id],
    queryFn: () => api<ChangeDetail>('GET', `/api/changes/${encodeURIComponent(id)}`),
    refetchInterval: POLL_MS,
  });

export function createRole(name: string) {
  return api<Role>('POST', '/api/roles', { name });
}

/** Adds an edit to the draft change, which the controller creates when there is none. */
export function addEdit(edit: Edit) {
  return api<Change>('POST', '/api/changes/draft/edits', edit);
}

export function discardChange(id: string) {
  return api<Change>('POST', `/api/changes/${encodeURIComponent(id)}/discard`);
}
