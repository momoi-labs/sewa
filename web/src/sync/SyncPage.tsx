import {
  Alert,
  AlertContent,
  AlertDescription,
  AlertTitle,
  Button,
  Card,
  CardContent,
  Checkbox,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Skeleton,
  Spinner,
  StepList,
  type StepItem,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { type ReactNode, useState } from 'react';

import { createSync, type Host, hostsQuery, syncEditsQuery, syncsQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { formatDateTime, osNames } from '../format';
import { HostStatusBadge } from '../hosts/HostStatusBadge';
import { byName } from './byPackage';
import { describeEdit, plural } from './sync';

const SHOWN_EDITS = 8;

/** Starts a sync: the edits since the last one, then the hosts to bring to
 *  their desired state. */
export function SyncPage() {
  const syncs = useQuery(syncsQuery);
  const running = syncs.data?.find((sync) => sync.status !== 'done');

  return (
    <>
      <PageHeader>
        <PageHeaderTitle>Sync</PageHeaderTitle>
        <PageHeaderDescription>
          Edits change the desired state only. A sync brings the hosts you pick to it: each host plans, you sign
          the plans with a passkey, and each host applies its own.
        </PageHeaderDescription>
      </PageHeader>
      {running && (
        <Alert variant="info">
          <AlertContent>
            <AlertTitle>A sync is under way</AlertTitle>
            <AlertDescription>
              Started {formatDateTime(running.created_at)} by {running.created_by}.{' '}
              <Link to="/sync/$syncId" params={{ syncId: running.id }}>
                Open it
              </Link>
            </AlertDescription>
          </AlertContent>
        </Alert>
      )}
      <Card>
        <CardContent>
          <NewSync />
        </CardContent>
      </Card>
      <RecentSyncs />
    </>
  );
}

function NewSync() {
  const edits = useQuery(syncEditsQuery);
  const hosts = useQuery(hostsQuery);
  const enrolled = (hosts.data ?? [])
    .filter((host) => host.enrolled_at !== null)
    .sort((a, b) => byName(a.name, b.name));
  const [picked, setPicked] = useState<Set<string> | null>(null);
  const chosen = picked ?? new Set(enrolled.map((host) => host.id));
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const mutation = useMutation({
    mutationFn: () => createSync(enrolled.filter((host) => chosen.has(host.id)).map((host) => host.id)),
    onSuccess: async (sync) => {
      queryClient.setQueryData(['syncs', sync.id], sync);
      await queryClient.invalidateQueries({ queryKey: ['syncs'] });
      await navigate({ to: '/sync/$syncId', params: { syncId: sync.id } });
    },
  });

  const list = edits.data?.edits ?? [];
  const since = edits.data?.since;
  const count = enrolled.filter((host) => chosen.has(host.id)).length;

  const steps: StepItem[] = [
    {
      key: 'edits',
      label: 'Edits since the last sync',
      state: 'done',
      meta: edits.data ? (list.length ? plural(list.length, 'edit') : 'None') : undefined,
      detail: edits.isPending ? (
        <Skeleton />
      ) : list.length === 0 ? (
        <p className="muted">
          {since ? `No edits since the sync of ${formatDateTime(since)}.` : 'No edits yet.'} A sync still brings back
          a host that drifted.
        </p>
      ) : (
        <ul className="stack-sm">
          {list.slice(-SHOWN_EDITS).map((edit) => (
            <li key={edit.id}>{describeEdit(edit)}</li>
          ))}
          {list.length > SHOWN_EDITS && <li className="muted">and {plural(list.length - SHOWN_EDITS, 'earlier edit')}</li>}
        </ul>
      ),
    },
    {
      key: 'hosts',
      label: 'Pick hosts',
      state: 'running',
      meta: hosts.data ? `${count} of ${enrolled.length}` : undefined,
      detail: hosts.isPending ? (
        <Skeleton />
      ) : enrolled.length === 0 ? (
        <p className="muted">
          No host is enrolled yet. <Link to="/">Add a host</Link> first.
        </p>
      ) : (
        <HostPicker
          hosts={enrolled}
          chosen={chosen}
          onChange={setPicked}
          action={
            <Button
              type="button"
              variant="primary"
              disabled={count === 0}
              aria-busy={mutation.isPending}
              onClick={() => {
                if (!mutation.isPending) mutation.mutate();
              }}
            >
              {mutation.isPending && <Spinner size="sm" />}
              Plan {plural(count, 'host')}
            </Button>
          }
        />
      ),
    },
    { key: 'plans', label: 'Plans', state: 'pending' },
    { key: 'sign', label: 'Sign', state: 'pending' },
    { key: 'apply', label: 'Apply', state: 'pending' },
  ];

  return (
    <div className="stack">
      {edits.isError && <ErrorAlert error={edits.error} thing="edits" onRetry={() => void edits.refetch()} />}
      {mutation.isError && <ErrorAlert error={mutation.error} thing="sync" />}
      <StepList label="Sync progress" steps={steps} />
    </div>
  );
}

function HostPicker({
  hosts,
  chosen,
  onChange,
  action,
}: {
  hosts: Host[];
  chosen: Set<string>;
  onChange: (chosen: Set<string>) => void;
  action: ReactNode;
}) {
  const pick = (filter: (host: Host) => boolean) => onChange(new Set(hosts.filter(filter).map((host) => host.id)));
  return (
    <div className="stack-sm">
      <div className="row-wrap">
        <Button type="button" size="sm" onClick={() => pick(() => true)}>
          All
        </Button>
        <Button type="button" size="sm" onClick={() => pick((host) => host.os === 'linux')}>
          All Linux
        </Button>
        <Button type="button" size="sm" onClick={() => pick((host) => host.os === 'macos')}>
          All macOS
        </Button>
        <Button type="button" size="sm" onClick={() => pick(() => false)}>
          None
        </Button>
      </div>
      <ul className="host-picker" aria-label="Hosts to sync">
        {hosts.map((host) => (
          <li key={host.id}>
            <label className="host-picker-row">
              <Checkbox
                checked={chosen.has(host.id)}
                onCheckedChange={(checked) => {
                  const next = new Set(chosen);
                  if (checked === true) next.add(host.id);
                  else next.delete(host.id);
                  onChange(next);
                }}
              />
              <span className="host-picker-name">{host.name}</span>
              <span className="muted">{osNames[host.os]}</span>
              <HostStatusBadge status={host.status} />
            </label>
          </li>
        ))}
      </ul>
      <div className="row-wrap">{action}</div>
    </div>
  );
}

const statusNames = { planning: 'Planning', ready: 'Waiting for a signature', applying: 'Applying', done: 'Done' };

function RecentSyncs() {
  const syncs = useQuery(syncsQuery);
  if (!syncs.data?.length) return null;
  return (
    <section className="stack-sm">
      <h2 className="t-caps">Recent syncs</h2>
      <TableFrame>
        <Table aria-label="Recent syncs">
          <TableHeader>
            <TableRow>
              <TableHead>Started</TableHead>
              <TableHead>By</TableHead>
              <TableHead>Hosts</TableHead>
              <TableHead>Status</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {syncs.data.map((sync) => (
              <TableRow key={sync.id}>
                <TableCell>
                  <Link to="/sync/$syncId" params={{ syncId: sync.id }}>
                    {formatDateTime(sync.created_at)}
                  </Link>
                </TableCell>
                <TableCell>{sync.created_by}</TableCell>
                <TableCell>{sync.host_count}</TableCell>
                <TableCell>{statusNames[sync.status]}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableFrame>
    </section>
  );
}
