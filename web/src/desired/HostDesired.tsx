import {
  Badge,
  Button,
  EmptyState,
  EmptyStateDescription,
  EmptyStateTitle,
  Form,
  FormActions,
  KV,
  KVKey,
  KVValue,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Spinner,
  StatusBadge,
  type StatusTone,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { type FormEvent, type ReactNode, useState } from 'react';

import {
  createSync,
  editHost,
  type Host,
  type HostEdit,
  type HostState,
  ignoreDrift,
  type Item,
  type Named,
  rolesQuery,
} from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { Unknown } from '../components/Unknown';
import { formatDateTime } from '../format';
import { AdoptDialog } from './AdoptDialog';
import { ItemChips } from './ItemChips';
import { itemKey, itemProblem } from './items';
import { PlanView } from './PlanView';
import { namesOf } from './RolePage';

const adjustmentProblem = (item: Item) => (item.version === 'absent' ? null : itemProblem(item));

function chipsOf(state: HostState): Item[] {
  return state.adjustments.map((adjustment) => ({
    kind: adjustment.kind,
    name: adjustment.name,
    version: adjustment.action === 'set' ? adjustment.version : 'absent',
    os: null,
  }));
}

function editsOf(state: HostState, roles: Named[], items: Item[]): HostEdit[] {
  const edits: HostEdit[] = [];
  const before = new Set(state.roles.map((role) => role.id));
  const after = new Set(roles.map((role) => role.id));
  for (const role of roles) if (!before.has(role.id)) edits.push({ op: 'host_role_add', role: role.id });
  for (const role of state.roles) if (!after.has(role.id)) edits.push({ op: 'host_role_remove', role: role.id });
  const saved = new Map(chipsOf(state).map((item) => [itemKey(item), item]));
  const kept = new Set(items.map(itemKey));
  for (const item of items) {
    if (saved.get(itemKey(item))?.version === item.version) continue;
    const { kind, name, version } = item;
    edits.push(version === 'absent' ? { op: 'host_item_remove', kind, name } : { op: 'host_item_set', kind, name, version });
  }
  for (const item of saved.values())
    if (!kept.has(itemKey(item))) edits.push({ op: 'host_item_reset', kind: item.kind, name: item.name });
  return edits;
}

export function HostDesired({ host, state }: { host: Host; state: HostState }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const roles = useQuery(rolesQuery);
  const [draftRoles, setDraftRoles] = useState<Named[] | null>(null);
  const [draftItems, setDraftItems] = useState<Item[] | null>(null);
  const [chosen, setChosen] = useState('');
  const given = draftRoles ?? state.roles;
  const items = draftItems ?? chipsOf(state);
  const edits = editsOf(state, given, items);
  const dirty = edits.length > 0;
  const ready = items.every((item) => adjustmentProblem(item) === null);

  const save = useMutation({
    mutationFn: () => editHost(host.id, edits),
    onSuccess: async (next) => {
      queryClient.setQueryData(['hosts', host.id, 'state'], next);
      setDraftRoles(null);
      setDraftItems(null);
      await Promise.all([['roles'], ['syncs']].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
      toast('success', `${host.name} saved`, 'Sync the host to apply it.');
    },
  });

  const builtin = new Set((roles.data ?? []).filter((role) => role.builtin).map((role) => role.id));
  const have = new Set(given.map((role) => role.id));
  const others = (roles.data ?? []).filter((role) => !have.has(role.id) && !role.builtin);

  function discard() {
    setDraftRoles(null);
    setDraftItems(null);
    save.reset();
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (dirty && ready && !save.isPending) save.mutate();
  }

  function give() {
    const role = others.find((role) => role.id === chosen);
    if (!role) return;
    setDraftRoles([...given, { id: role.id, name: role.name }]);
    setChosen('');
  }

  let message: ReactNode;
  if (dirty)
    message = (
      <>
        <strong>Unsaved changes.</strong>
      </>
    );
  else message = 'Saved. The host gets it at its next sync.';

  return (
    <Form onSubmit={submit} noValidate>
      <div className="form-body">
        {save.isError && <ErrorAlert error={save.error} />}
        <fieldset className="form-lock" disabled={save.isPending}>
        <ol className="form-steps">
          <li data-done>
            <p className="t-caps">Roles</p>
            {given.length === 0 ? (
              <p className="muted">This host has no roles.</p>
            ) : (
              <ul className="roles-list" aria-label={`Roles of ${host.name}`}>
                {given.map((role) => (
                  <li key={role.id} className="row-wrap">
                    <Link to="/roles/$roleId" params={{ roleId: role.id }}>
                      {role.name}
                    </Link>
                    {builtin.has(role.id) ? (
                      <Badge variant="neutral">Built in</Badge>
                    ) : (
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        aria-label={`Take away ${role.name}`}
                        onClick={() => setDraftRoles(given.filter((other) => other.id !== role.id))}
                      >
                        Take away
                      </Button>
                    )}
                  </li>
                ))}
              </ul>
            )}
            {others.length > 0 && (
              <div className="row-wrap">
                <Select value={chosen} onValueChange={setChosen}>
                  <SelectTrigger aria-label="Role to give">
                    <SelectValue placeholder="Choose a role" />
                  </SelectTrigger>
                  <SelectContent>
                    {others.map((role) => (
                      <SelectItem key={role.id} value={role.id}>
                        {role.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button type="button" disabled={!chosen} onClick={give}>
                  Give role
                </Button>
              </div>
            )}
          </li>
          <li data-done>
            <p className="t-caps">Adjustments</p>
            <p className="muted">
              An adjustment adds an item to this host, changes its version, or leaves out an item of a role.
            </p>
            <ItemChips
              value={items}
              onChange={setDraftItems}
              problem={adjustmentProblem}
              withAbsent
              suggestions={{
                package: namesOf(roles.data, null, 'package'),
                tool: namesOf(roles.data, null, 'tool'),
              }}
            />
          </li>
        </ol>
        </fieldset>
      </div>
      <FormActions sticky tone={dirty ? 'warning' : 'neutral'} message={message}>
        {dirty && (
          <Button type="button" size="sm" onClick={discard} disabled={save.isPending}>
            Discard
          </Button>
        )}
        <Button type="submit" size="sm" variant="primary" disabled={!dirty || !ready} aria-busy={save.isPending}>
          {save.isPending && <Spinner size="sm" />}
          Save
        </Button>
      </FormActions>
    </Form>
  );
}

const health: Record<string, { tone: StatusTone; label: string }> = {
  in_sync: { tone: 'success', label: 'In sync' },
  drift: { tone: 'danger', label: 'Drift' },
  error: { tone: 'danger', label: 'Plan failed' },
};

export function HostDrift({ host, state }: { host: Host; state: HostState }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const toast = useToast();
  const [adopting, setAdopting] = useState<string | null>(null);
  const drift = state.drift;
  const status = drift ? health[drift.status] : null;
  const undeclared = state.undeclared;
  const revert = useMutation({
    mutationFn: () => createSync([host.id]),
    onSuccess: async (sync) => {
      queryClient.setQueryData(['syncs', sync.id], sync);
      await queryClient.invalidateQueries({ queryKey: ['syncs'] });
      await navigate({ to: '/sync/$syncId', params: { syncId: sync.id } });
    },
  });
  const ignore = useMutation({
    mutationFn: () => ignoreDrift(host.id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['hosts'] });
      toast('success', 'Drift ignored', 'It shows again when the drift changes or another package appears.');
    },
  });
  const drifted = drift?.status === 'drift' && !drift.ignored;
  const hasUndeclared = (undeclared?.packages.length ?? 0) > 0;
  return (
    <div className="stack">
      {(revert.isError || ignore.isError) && <ErrorAlert error={revert.error ?? ignore.error} />}
      <KV>
        <KVKey>Health</KVKey>
        <KVValue>
          {status ? (
            <div className="row-wrap">
              <StatusBadge tone={drift?.ignored ? 'neutral' : status.tone}>
                {drift?.ignored ? `${status.label}, ignored` : status.label}
              </StatusBadge>
            </div>
          ) : (
            <Unknown reason="The agent has not planned the desired state yet" />
          )}
        </KVValue>
        <KVKey>Last plan</KVKey>
        <KVValue>
          {drift ? formatDateTime(drift.reported_at) : <Unknown reason="The agent has not planned it yet" />}
        </KVValue>
      </KV>

      {(drift?.status === 'drift' || hasUndeclared) && (
        <div className="row-wrap">
          {drift?.status === 'drift' && (
              <Button
                size="sm"
                variant="primary"
                aria-busy={revert.isPending}
                onClick={() => {
                  if (!revert.isPending) revert.mutate();
                }}
              >
                {revert.isPending && <Spinner size="sm" />}
                Revert
              </Button>
          )}
          {(drifted || hasUndeclared) && (
            <Button
              size="sm"
              aria-busy={ignore.isPending}
              onClick={() => {
                if (!ignore.isPending) ignore.mutate();
              }}
            >
              {ignore.isPending && <Spinner size="sm" />}
              Ignore
            </Button>
          )}
        </div>
      )}

      <h3 className="t-h3">Plan to fix the drift</h3>
      <p className="muted">
        The agent plans the desired state on start and once a day. Revert starts a sync of this host, which applies
        this plan once you sign it.
        {drift?.ignored && ' Ignore hid this drift until it changes.'}
      </p>
      <PlanView plan={drift} waiting="The agent plans the desired state when it starts, then once a day." />

      <h3 className="t-h3">Undeclared packages</h3>
      {undeclared === null ? (
        <EmptyState variant="informational">
          <EmptyStateTitle>No package list yet</EmptyStateTitle>
          <EmptyStateDescription>
            The agent sends the explicitly installed packages with its next plan.
          </EmptyStateDescription>
        </EmptyState>
      ) : (
        <>
          <p className="muted">
            {undeclared.packages.length} of {undeclared.explicit_count} packages installed explicitly with{' '}
            <span className="mono">{undeclared.manager}</span> are in no role or adjustment. Listed{' '}
            {formatDateTime(undeclared.reported_at)}.
            {undeclared.ignored_count > 0 &&
              ` Ignore hides ${undeclared.ignored_count === 1 ? '1 more' : `${undeclared.ignored_count} more`}.`}
          </p>
          {undeclared.packages.length > 0 && (
            <TableFrame frame="none">
              <Table aria-label="Undeclared packages">
                <TableHeader>
                  <TableRow>
                    <TableHead>Package</TableHead>
                    <TableHead>Manager</TableHead>
                    <TableHead aria-label="Actions" />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {undeclared.packages.map((name) => (
                    <TableRow key={name}>
                      <TableCell className="mono">{name}</TableCell>
                      <TableCell className="mono">{undeclared.manager}</TableCell>
                      <TableCell>
                        <Button size="sm" variant="ghost" aria-label={`Adopt ${name}`} onClick={() => setAdopting(name)}>
                          Adopt
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TableFrame>
          )}
        </>
      )}
      <AdoptDialog host={host} pkg={adopting} onClose={() => setAdopting(null)} />
    </div>
  );
}
