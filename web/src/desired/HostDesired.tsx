import {
  Badge,
  Button,
  EmptyState,
  EmptyStateDescription,
  EmptyStateTitle,
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
import { useState } from 'react';

import { createSync, editHost, type Host, type HostEdit, type HostState, ignoreDrift, rolesQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { Unknown } from '../components/Unknown';
import { formatDateTime } from '../format';
import { AdoptDialog } from './AdoptDialog';
import { ItemForm } from './ItemForm';
import { kindNames } from './items';
import { PlanView } from './PlanView';

/** Edits the host's roles and adjustments. They change its desired state
 *  only: the host gets them at its next sync. */
function useHostEdit(host: Host) {
  const queryClient = useQueryClient();
  const toast = useToast();
  return useMutation({
    mutationFn: (edit: HostEdit) => editHost(host.id, [edit]),
    onSuccess: async (state) => {
      queryClient.setQueryData(['hosts', host.id, 'state'], state);
      await Promise.all([['roles'], ['syncs']].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
      toast('success', `${host.name} saved`, 'Sync the host to apply it.');
    },
  });
}

/** The host's roles, its adjustments, and the items they add up to. Each
 *  edit saves at once and applies at the next sync. */
export function HostDesired({ host, state }: { host: Host; state: HostState }) {
  const edit = useHostEdit(host);
  const roles = useQuery(rolesQuery);
  const [chosen, setChosen] = useState('');
  const builtin = new Set((roles.data ?? []).filter((role) => role.builtin).map((role) => role.id));
  const have = new Set(state.roles.map((role) => role.id));
  const others = (roles.data ?? []).filter((role) => !have.has(role.id) && !role.builtin);

  return (
    <div className="stack">
      <p className="muted">Edits save at once. The host gets them at its next sync.</p>
      {edit.isError && <ErrorAlert error={edit.error} />}

      <h3 className="t-h3">Roles</h3>
      {state.roles.length === 0 ? (
        <p className="muted">This host has no roles.</p>
      ) : (
        <TableFrame frame="none">
          <Table aria-label={`Roles of ${host.name}`}>
            <TableHeader>
              <TableRow>
                <TableHead>Role</TableHead>
                <TableHead aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {state.roles.map((role) => (
                <TableRow key={role.id}>
                  <TableCell>
                    <div className="row-wrap">
                      <Link to="/roles/$roleId" params={{ roleId: role.id }}>
                        {role.name}
                      </Link>
                      {builtin.has(role.id) && <Badge variant="neutral">Built in</Badge>}
                    </div>
                  </TableCell>
                  <TableCell>
                    {!builtin.has(role.id) && (
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`Take away ${role.name}`}
                        disabled={edit.isPending}
                        onClick={() => edit.mutate({ op: 'host_role_remove', role: role.id })}
                      >
                        Take away
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
      {others.length > 0 && (
        <form
          className="row-wrap"
          onSubmit={(event) => {
            event.preventDefault();
            if (!chosen) return;
            edit.mutate({ op: 'host_role_add', role: chosen });
            setChosen('');
          }}
        >
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
          <Button type="submit" disabled={!chosen || edit.isPending}>
            Give role
          </Button>
        </form>
      )}

      <h3 className="t-h3">Adjustments</h3>
      <p className="muted">
        An adjustment adds an item to this host, changes its version, or leaves out an item of a role.
      </p>
      {state.adjustments.length > 0 && (
        <TableFrame frame="none">
          <Table aria-label={`Adjustments of ${host.name}`}>
            <TableHeader>
              <TableRow>
                <TableHead>Item</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>Version</TableHead>
                <TableHead aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {state.adjustments.map((adjustment) => (
                <TableRow key={`${adjustment.kind}:${adjustment.name}`}>
                  <TableCell className="mono">{adjustment.name}</TableCell>
                  <TableCell>{kindNames[adjustment.kind]}</TableCell>
                  <TableCell className="mono">
                    {adjustment.action === 'set' ? adjustment.version : <span className="muted">left out</span>}
                  </TableCell>
                  <TableCell>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Reset ${adjustment.name}`}
                      disabled={edit.isPending}
                      onClick={() =>
                        edit.mutate({ op: 'host_item_reset', kind: adjustment.kind, name: adjustment.name })
                      }
                    >
                      Reset
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
      <ItemForm submitLabel="Add" onAdd={(item) => edit.mutateAsync({ op: 'host_item_set', ...item })} />

      <h3 className="t-h3">Desired items</h3>
      {state.items.length === 0 ? (
        <p className="muted">This host has no desired items.</p>
      ) : (
        <TableFrame frame="none">
          <Table aria-label={`Desired items of ${host.name}`}>
            <TableHeader>
              <TableRow>
                <TableHead>Item</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>Version</TableHead>
                <TableHead aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {state.items.map((item) => (
                <TableRow key={`${item.kind}:${item.name}`}>
                  <TableCell className="mono">{item.name}</TableCell>
                  <TableCell>{kindNames[item.kind]}</TableCell>
                  <TableCell className="mono">{item.version}</TableCell>
                  <TableCell>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Leave out ${item.name}`}
                      disabled={edit.isPending}
                      onClick={() => edit.mutate({ op: 'host_item_remove', kind: item.kind, name: item.name })}
                    >
                      Leave out
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
    </div>
  );
}

const health: Record<string, { tone: StatusTone; label: string }> = {
  in_sync: { tone: 'success', label: 'In sync' },
  drift: { tone: 'danger', label: 'Drift' },
  error: { tone: 'danger', label: 'Plan failed' },
};

/** Whether the host matches its desired state, the plan that would fix
 *  it, and the packages installed outside any role. Revert, Adopt and
 *  Ignore act on the drift. */
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
