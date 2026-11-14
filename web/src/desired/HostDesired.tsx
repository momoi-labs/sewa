import {
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

import {
  type Adjustment,
  type Host,
  type HostState,
  ignoreDrift,
  type Item,
  revertDrift,
  rolesQuery,
} from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { Unknown } from '../components/Unknown';
import { formatDateTime } from '../format';
import { AdoptDialog } from './AdoptDialog';
import { compareItems, compareNamed } from './compare';
import { DifferenceBadge } from './DifferenceBadge';
import { DraftNotice } from './DraftNotice';
import { ItemForm } from './ItemForm';
import { ItemsTable } from './ItemsTable';
import { PlanView } from './PlanView';
import { useEdit } from './useEdit';

const kinds = { package: 'Package', tool: 'mise tool' } as const;

/** An adjustment as an item, with "left out" as the version of a removal. */
function asItem(adjustment: Adjustment): Item {
  return {
    kind: adjustment.kind,
    name: adjustment.name,
    version: adjustment.action === 'set' ? adjustment.version : 'left out',
  };
}

/** The host's roles, its adjustments, and the items they add up to. Every
 *  edit goes into the draft change. */
export function HostDesired({ host, state }: { host: Host; state: HostState }) {
  const edit = useEdit();
  const roles = useQuery(rolesQuery);
  const [chosen, setChosen] = useState('');
  const roleRows = compareNamed(state.roles, state.proposed_roles);
  const have = new Set(roleRows.filter((row) => row.difference !== 'removed').map((row) => row.id));
  const others = (roles.data ?? []).filter((role) => !have.has(role.id));
  const adjustmentRows = compareItems(state.adjustments.map(asItem), state.proposed_adjustments?.map(asItem) ?? null);

  return (
    <div className="stack">
      <DraftNotice />
      {edit.isError && <ErrorAlert error={edit.error} />}

      <h3 className="t-h3">Roles</h3>
      {roleRows.length === 0 ? (
        <p className="muted">This host has no roles.</p>
      ) : (
        <TableFrame frame="none">
          <Table aria-label={`Roles of ${host.name}`}>
            <TableHeader>
              <TableRow>
                <TableHead>Role</TableHead>
                <TableHead>Draft</TableHead>
                <TableHead aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {roleRows.map((row) => (
                <TableRow key={row.id}>
                  <TableCell>
                    <Link to="/roles/$roleId" params={{ roleId: row.id }}>
                      {row.name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <DifferenceBadge difference={row.difference} />
                  </TableCell>
                  <TableCell>
                    {row.difference !== 'removed' && (
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`Take away ${row.name}`}
                        onClick={() =>
                          edit.mutate({
                            op: 'host_role_remove',
                            host: host.id,
                            role: row.id,
                          })
                        }
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
            edit.mutate({ op: 'host_role_add', host: host.id, role: chosen });
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
          <Button type="submit" disabled={!chosen}>
            Give role in draft
          </Button>
        </form>
      )}

      <h3 className="t-h3">Adjustments</h3>
      <p className="muted">
        An adjustment adds an item to this host, changes its version, or leaves out an item of a role.
      </p>
      {adjustmentRows.length > 0 && (
        <ItemsTable
          rows={adjustmentRows}
          label={`Adjustments of ${host.name}`}
          removeLabel="Reset"
          onRemove={(row) =>
            edit.mutate({
              op: 'host_item_reset',
              host: host.id,
              kind: row.kind,
              name: row.name,
            })
          }
        />
      )}
      <ItemForm
        submitLabel="Add to draft"
        onAdd={(item) => edit.mutateAsync({ op: 'host_item_set', host: host.id, ...item })}
      />

      <h3 className="t-h3">Approved items</h3>
      {state.items.length === 0 ? (
        <p className="muted">No items are approved for this host.</p>
      ) : (
        <TableFrame frame="none">
          <Table aria-label={`Approved items of ${host.name}`}>
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
                  <TableCell>{kinds[item.kind]}</TableCell>
                  <TableCell className="mono">{item.version}</TableCell>
                  <TableCell>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Leave out ${item.name}`}
                      onClick={() =>
                        edit.mutate({
                          op: 'host_item_remove',
                          host: host.id,
                          kind: item.kind,
                          name: item.name,
                        })
                      }
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

/** Whether the host matches its approved state, the plan that would fix
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
    mutationFn: () => revertDrift(host.id),
    onSuccess: async (change) => {
      await queryClient.invalidateQueries({ queryKey: ['changes'] });
      await navigate({ to: '/changes/$changeId', params: { changeId: change.id } });
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
            <Unknown reason="The agent has not planned the approved state yet" />
          )}
        </KVValue>
        <KVKey>Last plan</KVKey>
        <KVValue>
          {drift ? formatDateTime(drift.reported_at) : <Unknown reason="The agent has not planned it yet" />}
        </KVValue>
      </KV>

      {(drift?.status === 'drift' || hasUndeclared || drift?.revert_change) && (
        <div className="row-wrap">
          {drift?.revert_change ? (
            <Link to="/changes/$changeId" params={{ changeId: drift.revert_change }}>
              Open the revert change
            </Link>
          ) : (
            drift?.status === 'drift' && (
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
            )
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
        The agent plans the approved state on start and once a day. Revert opens a change that applies this plan once
        you approve it.
        {drift?.ignored && ' Ignore hid this drift until it changes.'}
      </p>
      <PlanView plan={drift} waiting="The agent plans the approved state when it starts, then once a day." />

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
