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
  /** Null until the agent connects and reports it. */
  os: Os | null;
  status: HostStatus;
  created_at: string;
  enrolled_at: string | null;
  last_seen_at: string | null;
  agent: Agent | null;
};

/** An item of `GET /api/hosts`: the host, and its roles without the built-in one. */
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
export async function api<T>(method: 'GET' | 'POST' | 'PUT', path: string, body?: unknown): Promise<T> {
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

/** True when a failed load should take the whole page: the person is not an
 *  admin, the controller failed, or it did not answer. */
export function isPageError(error: unknown): boolean {
  if (!(error instanceof ApiFailure)) return true;
  return error.code === 'not_admin' || error.code === 'network' || error.status >= 500;
}

/** A 403 does not change by itself, so it is neither retried nor polled again. */
function isForbidden(error: unknown): boolean {
  return error instanceof ApiFailure && error.status === 403;
}

/** React Query's default retry (3 times), except for a 403. */
export function retryQuery(failureCount: number, error: unknown): boolean {
  return !isForbidden(error) && failureCount < 3;
}

function poll(query: { state: { error: unknown } }): number | false {
  return isForbidden(query.state.error) ? false : POLL_MS;
}

/** Query defaults. A failed first load that takes the page goes to the root
 *  error boundary; a failed refresh of data already on screen does not. */
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

export type ItemKind = 'package' | 'tool';

/** A system package (a mise bootstrap key) or a mise tool, its version, and
 *  the one OS that gets it, or null for every OS. */
export type Item = { kind: ItemKind; name: string; version: string; os?: Os | null };

export type Named = { id: string; name: string };

export type Role = {
  id: string;
  name: string;
  /** The built-in role: every host has it, and sewa manages it, so it is read-only. */
  builtin: boolean;
  created_at: string;
  items: Item[];
  hosts: Named[];
};

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
  /** When the agent planned, by its clock. */
  reported_at: string;
  /** When the controller got the plan, if late: the agent kept it while it could not reach the controller. */
  received_at: string | null;
  /** The plan is of an older config; a new one is on its way. */
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
        /** Ignore hid this drift. */
        ignored: boolean;
      })
    | null;
  undeclared: {
    manager: string;
    packages: string[];
    /** Undeclared packages that Ignore hid. */
    ignored_count: number;
    explicit_count: number;
    reported_at: string;
  } | null;
};

/** One edit of a host's roles or adjustments. */
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

/** Changes the host's roles and adjustments at once. The host gets them at its next sync. */
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

/** Replaces the role's name and items. */
export function saveRole(id: string, name: string, items: Item[]) {
  return api<Role>('PUT', `/api/roles/${encodeURIComponent(id)}`, { name, items });
}

/** Adds an undeclared package to a role, or to the host's adjustments when `role` is null. */
export function adoptPackage(host: string, pkg: string, role: string | null) {
  return api<null>('POST', `/api/hosts/${encodeURIComponent(host)}/drift/adopt`, { package: pkg, role });
}

export function ignoreDrift(host: string) {
  return api<null>('POST', `/api/hosts/${encodeURIComponent(host)}/drift/ignore`);
}

/** Where a host is in a sync. `replan`: its plan changed before it applied, so it needs a new signature. */
export type SyncStep = 'picked' | 'in_sync' | 'skipped' | 'waiting' | 'applied' | 'failed' | 'replan';

export type SyncStatus = 'planning' | 'ready' | 'applying' | 'done';

export type HostResult = {
  plan_hash: string | null;
  log: string | null;
  /** When the agent finished, by its clock. */
  reported_at: string | null;
  /** When the controller got the result, if late, as with a plan. */
  received_at: string | null;
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

export type Instance = { address: string };

export const instanceQuery = queryOptions({
  queryKey: ['instance'],
  queryFn: () => api<Instance>('GET', '/api/instance'),
  staleTime: Infinity,
});

export type Passkey = { id: string; login: string; name: string; credential_id: string; created_at: string };

export const passkeysQuery = queryOptions({
  queryKey: ['passkeys'],
  queryFn: () => api<Passkey[]>('GET', '/api/passkeys'),
});

/** What the browser needs to create a passkey. Bytes are unpadded base64url. */
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

/** What a signature of a sync signs now. `change_id` holds the sync id. */
export type ApprovalOptions = {
  challenge: string;
  rp_id: string;
  allow_credentials: string[];
  signed_text: string;
  change_id: string;
  hosts: HostPlan[];
};

export type Assertion = {
  credential_id: string;
  authenticator_data: string;
  client_data_json: string;
  signature: string;
};

/** What a signature signs now, for every host with changes or only `hosts`. */
export function syncApprovalOptions(sync: string, hosts?: string[]) {
  return api<ApprovalOptions>('POST', `/api/syncs/${encodeURIComponent(sync)}/approval`, hosts ? { hosts } : undefined);
}

export function approveSync(sync: string, assertion: Assertion, hosts?: string[]) {
  return api<Sync>('POST', `/api/syncs/${encodeURIComponent(sync)}/approve`, { ...assertion, hosts: hosts ?? null });
}
