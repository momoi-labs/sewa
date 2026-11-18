import { queryOptions } from '@tanstack/react-query';

export const POLL_MS = 5000;

export type HostStatus = 'waiting' | 'connected' | 'disconnected' | 'disabled';
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
  os: Os | null;
  status: HostStatus;
  created_at: string;
  disabled_at: string | null;
  enrolled_at: string | null;
  last_seen_at: string | null;
  agent: Agent | null;
};

export type ListedHost = Host & { roles: Named[] };

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

export class ApiFailure extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string) {
    super(`API call failed: ${status} ${code}`);
    this.status = status;
    this.code = code;
  }
}

export async function api<T>(method: 'GET' | 'POST' | 'PUT' | 'DELETE', path: string, body?: unknown): Promise<T> {
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

export function isPageError(error: unknown): boolean {
  if (!(error instanceof ApiFailure)) return true;
  return error.code === 'not_admin' || error.code === 'network' || error.status >= 500;
}

function isForbidden(error: unknown): boolean {
  return error instanceof ApiFailure && error.status === 403;
}

export function retryQuery(failureCount: number, error: unknown): boolean {
  return !isForbidden(error) && failureCount < 3;
}

function poll(query: { state: { error: unknown } }): number | false {
  return isForbidden(query.state.error) ? false : POLL_MS;
}

export const queryDefaults = {
  queries: {
    retry: retryQuery,
    throwOnError: (error: unknown, query: { state: { data: unknown } }) =>
      query.state.data === undefined && isPageError(error),
  },
};

export const hostsQuery = queryOptions({
  queryKey: ['hosts'],
  queryFn: () => api<ListedHost[]>('GET', '/api/hosts'),
  refetchInterval: poll,
});

export const hostQuery = (id: string) =>
  queryOptions({
    queryKey: ['hosts', id],
    queryFn: () => api<Host>('GET', `/api/hosts/${encodeURIComponent(id)}`),
    refetchInterval: poll,
  });

export const eventsQuery = (id: string) =>
  queryOptions({
    queryKey: ['hosts', id, 'events'],
    queryFn: () => api<HostEvent[]>('GET', `/api/hosts/${encodeURIComponent(id)}/events`),
    refetchInterval: poll,
  });

export function createHost(name: string) {
  return api<Created>('POST', '/api/hosts', { name });
}

export function disableHost(id: string) {
  return api<Host>('POST', `/api/hosts/${encodeURIComponent(id)}/disable`);
}

export function enableHost(id: string) {
  return api<Created>('POST', `/api/hosts/${encodeURIComponent(id)}/enable`);
}

export function deleteHost(id: string) {
  return api<null>('DELETE', `/api/hosts/${encodeURIComponent(id)}`);
}

export type ItemKind = 'package' | 'tool';

export type Item = { kind: ItemKind; name: string; version: string; os?: Os | null };

export type Named = { id: string; name: string };

export type Role = {
  id: string;
  name: string;
  builtin: boolean;
  created_at: string;
  items: Item[];
  hosts: Named[];
};

export type PlanResource = {
  id: { kind: string; name: string };
  current?: string | null;
  desired?: string | null;
  action: 'noop' | 'create' | 'update' | 'remove' | 'unknown' | string;
};

export type Plan = {
  config_hash: string;
  plan_hash: string;
  exit_code: number;
  plan: { resources?: PlanResource[]; summary?: Record<string, number> } | null;
  error: string | null;
  reported_at: string;
  received_at: string | null;
  stale: boolean;
};

export type Adjustment = { kind: ItemKind; name: string } & (
  | { action: 'set'; version: string }
  | { action: 'remove' }
);

export type HostState = {
  roles: Named[];
  adjustments: Adjustment[];
  items: Item[];
  config: string;
  health: string | null;
  drift:
    | (Plan & {
        status: 'in_sync' | 'drift' | 'error';
        ignored: boolean;
      })
    | null;
  undeclared: {
    manager: string;
    packages: string[];
    ignored_count: number;
    explicit_count: number;
    reported_at: string;
  } | null;
};

export type HostEdit =
  | { op: 'host_role_add'; role: string }
  | { op: 'host_role_remove'; role: string }
  | { op: 'host_item_set'; kind: ItemKind; name: string; version: string }
  | { op: 'host_item_remove'; kind: ItemKind; name: string }
  | { op: 'host_item_reset'; kind: ItemKind; name: string };

export const hostStateQuery = (id: string) =>
  queryOptions({
    queryKey: ['hosts', id, 'state'],
    queryFn: () => api<HostState>('GET', `/api/hosts/${encodeURIComponent(id)}/state`),
    refetchInterval: poll,
  });

export function editHost(host: string, edits: HostEdit[]) {
  return api<HostState>('POST', `/api/hosts/${encodeURIComponent(host)}/edits`, { edits });
}

export const rolesQuery = queryOptions({
  queryKey: ['roles'],
  queryFn: () => api<Role[]>('GET', '/api/roles'),
  refetchInterval: poll,
});

export const roleQuery = (id: string) =>
  queryOptions({
    queryKey: ['roles', id],
    queryFn: () => api<Role>('GET', `/api/roles/${encodeURIComponent(id)}`),
  });

export function createRole(name: string) {
  return api<Role>('POST', '/api/roles', { name });
}

export function saveRole(id: string, name: string, items: Item[]) {
  return api<Role>('PUT', `/api/roles/${encodeURIComponent(id)}`, { name, items });
}

export function adoptPackage(host: string, pkg: string, role: string | null) {
  return api<null>('POST', `/api/hosts/${encodeURIComponent(host)}/drift/adopt`, { package: pkg, role });
}

export function ignoreDrift(host: string) {
  return api<null>('POST', `/api/hosts/${encodeURIComponent(host)}/drift/ignore`);
}

export type SyncStep = 'picked' | 'in_sync' | 'skipped' | 'waiting' | 'applying' | 'applied' | 'failed' | 'replan';

export type SyncStatus = 'planning' | 'ready' | 'applying' | 'done';

export type HostResult = {
  plan_hash: string | null;
  log: string | null;
  reported_at: string | null;
  received_at: string | null;
};

export type ApplyProgress = {
  step: string | null;
  started_at: string;
  updated_at: string | null;
  steps: { step: string; at: string }[];
};

export type SyncHost = {
  id: string;
  name: string;
  os: Os | null;
  status: HostStatus;
  last_seen_at: string | null;
  roles: Named[];
  step: SyncStep;
  plan: Plan | null;
  result: HostResult | null;
  progress: ApplyProgress | null;
};

export type ApprovalEntry = { id: string; by: string; passkey: string; created_at: string; host_count: number };

export type Sync = {
  id: string;
  created_at: string;
  created_by: string;
  status: SyncStatus;
  hosts: SyncHost[];
  approvals: ApprovalEntry[];
};

export type SyncEntry = { id: string; created_at: string; created_by: string; status: SyncStatus; host_count: number };

export type EditEntry = {
  id: number;
  at: string;
  kind: string;
  role: Named | null;
  host: Named | null;
  data: Record<string, unknown>;
};

export type Edits = { since: string | null; edits: EditEntry[] };

export const syncsQuery = queryOptions({
  queryKey: ['syncs'],
  queryFn: () => api<SyncEntry[]>('GET', '/api/syncs'),
  refetchInterval: poll,
});

export const syncEditsQuery = queryOptions({
  queryKey: ['syncs', 'edits'],
  queryFn: () => api<Edits>('GET', '/api/syncs/edits'),
  refetchInterval: poll,
});

export const syncQuery = (id: string) =>
  queryOptions({
    queryKey: ['syncs', id],
    queryFn: () => api<Sync>('GET', `/api/syncs/${encodeURIComponent(id)}`),
    refetchInterval: poll,
  });

export function createSync(hosts: string[]) {
  return api<Sync>('POST', '/api/syncs', { hosts });
}

export type Passkey = { id: string; login: string; name: string; credential_id: string; created_at: string };

export const passkeysQuery = queryOptions({
  queryKey: ['passkeys'],
  queryFn: () => api<Passkey[]>('GET', '/api/passkeys'),
});

export type PasskeyOptions = {
  challenge: string;
  rp: { id: string; name: string };
  user: { id: string; name: string };
  algorithm: number;
  exclude_credentials: string[];
};

export type PasskeyRegistration = {
  name: string;
  credential_id: string;
  public_key: string;
  algorithm: number;
  authenticator_data: string;
  client_data_json: string;
};

export function passkeyOptions() {
  return api<PasskeyOptions>('POST', '/api/passkeys/options');
}

export function registerPasskey(registration: PasskeyRegistration) {
  return api<Passkey>('POST', '/api/passkeys', registration);
}

export type HostPlan = { host_id: string; instance_uid: string; config_hash: string; plan_hash: string };

export type ApprovalOptions = {
  challenge: string;
  rp_id: string;
  allow_credentials: string[];
  signed_text: string;
  approval_id: string;
  change_id: string;
  hosts: HostPlan[];
};

export type Assertion = {
  credential_id: string;
  authenticator_data: string;
  client_data_json: string;
  signature: string;
};

export function syncApprovalOptions(sync: string, hosts?: string[]) {
  return api<ApprovalOptions>('POST', `/api/syncs/${encodeURIComponent(sync)}/approval`, hosts ? { hosts } : undefined);
}

export function approveSync(sync: string, approvalId: string, assertion: Assertion, hosts?: string[]) {
  return api<Sync>('POST', `/api/syncs/${encodeURIComponent(sync)}/approve`, {
    ...assertion,
    approval_id: approvalId,
    hosts: hosts ?? null,
  });
}
