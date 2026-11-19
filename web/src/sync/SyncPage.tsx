import {
  Alert,
  AlertContent,
  AlertDescription,
  AlertTitle,
  Button,
  Card,
  Form,
  FormActions,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Skeleton,
  Spinner,
  StatusBadge,
  StepBar,
  type StepState,
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
import { type FormEvent, useState } from 'react';

import { createSync, hostsQuery, type SyncEntry, type SyncHost, syncEditsQuery, syncQuery, syncsQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { formatDateTime } from '../format';
import { byName } from './byPackage';
import { HostPicker } from './HostPicker';
import { describeEdit, plural } from './sync';

const SHOWN_EDITS = 8;

export function SyncPage() {
  const syncs = useQuery(syncsQuery);
  const running = syncs.data?.find((sync) => sync.status !== 'done');

  return (
    <>
      <PageHeader>
        <PageHeaderTitle>New sync</PageHeaderTitle>
        <PageHeaderDescription>
          Pick the hosts to bring to the desired state. Each host plans, you sign the plans with a passkey, and each
          host applies its own.
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
      <NewSync />
      <RecentSyncs />
    </>
  );
}

function NewSync() {
  const edits = useQuery(syncEditsQuery);
  const hosts = useQuery(hostsQuery);
  const enrolled = (hosts.data ?? [])
    .filter((host) => host.enrolled_at !== null && host.status !== 'disabled')
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

  function submit(event: FormEvent) {
    event.preventDefault();
    if (count > 0 && !mutation.isPending) mutation.mutate();
  }

  return (
    <Card className="form-page wide-form" data-fill="false">
      <Form onSubmit={submit} noValidate>
        <div className="form-body">
          {edits.isError && <ErrorAlert error={edits.error} thing="edits" onRetry={() => void edits.refetch()} />}
          {mutation.isError && <ErrorAlert error={mutation.error} thing="sync" />}
          <ol className="form-steps">
            <li data-done>
              <p className="t-caps">Edits since the last sync</p>
              {edits.isPending ? (
                <Skeleton />
              ) : list.length === 0 ? (
                <p className="muted">
                  {since ? `No edits since the sync of ${formatDateTime(since)}.` : 'No edits yet.'} A sync still
                  brings back a host that drifted.
                </p>
              ) : (
                <ul className="stack-sm">
                  {list.slice(-SHOWN_EDITS).map((edit) => (
                    <li key={edit.id}>{describeEdit(edit)}</li>
                  ))}
                  {list.length > SHOWN_EDITS && (
                    <li className="muted">and {plural(list.length - SHOWN_EDITS, 'earlier edit')}</li>
                  )}
                </ul>
              )}
            </li>
            <li data-done={count > 0 || undefined}>
              <p className="t-caps">Hosts</p>
              {hosts.isPending ? (
                <Skeleton />
              ) : enrolled.length === 0 ? (
                <p className="muted">
                  No host is enrolled yet. <Link to="/">Add a host</Link> first.
                </p>
              ) : (
                <HostPicker hosts={enrolled} chosen={chosen} onChange={setPicked} action={null} />
              )}
            </li>
          </ol>
        </div>
        <FormActions sticky message={`${count} of ${enrolled.length} hosts selected`}>
          <Button type="button" size="sm" onClick={() => void navigate({ to: '/' })}>
            Cancel
          </Button>
          <Button type="submit" size="sm" variant="primary" disabled={count === 0} aria-busy={mutation.isPending}>
            {mutation.isPending && <Spinner size="sm" />}
            Plan {plural(count, 'host')}
          </Button>
        </FormActions>
      </Form>
    </Card>
  );
}

const statusNames = { planning: 'Planning', ready: 'Waiting for a signature', applying: 'Applying', done: 'Done' };

function RecentSyncs() {
  const syncs = useQuery(syncsQuery);
  if (!syncs.data?.length) return null;
  return (
    <section className="stack-sm">
      <h2 className="t-caps">Recent syncs</h2>
      <TableFrame data-fill="false">
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
                <TableCell>
                  <SyncState sync={sync} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        <div className="table-footer">{plural(syncs.data.length, 'sync')}</div>
      </TableFrame>
    </section>
  );
}

function stepState(host: SyncHost, applying: boolean): StepState {
  if (host.step === 'applied' || host.step === 'in_sync') return 'done';
  if (host.step === 'skipped') return 'skipped';
  if (host.step === 'applying') return 'running';
  if (host.step === 'failed' || host.step === 'replan') return 'failed';
  if (host.step === 'waiting') return applying ? 'running' : 'pending';
  return applying ? 'pending' : 'running';
}

function SyncState({ sync }: { sync: SyncEntry }) {
  const done = sync.status === 'done';
  const detail = useQuery({ ...syncQuery(sync.id), enabled: !done });
  const badge = (
    <StatusBadge
      tone={done ? 'success' : 'neutral'}
      pulse={sync.status === 'planning' || sync.status === 'applying'}
    >
      {statusNames[sync.status]}
    </StatusBadge>
  );
  const hosts = detail.data?.hosts;
  if (done || !hosts?.length) return badge;
  const applying = sync.status === 'applying';
  const steps = hosts.map((host) => ({ key: host.id, label: host.name, state: stepState(host, applying) }));
  const finished = steps.filter((step) => step.state !== 'pending' && step.state !== 'running').length;
  return (
    <div className="row sync-state">
      {badge}
      <StepBar label={`${statusNames[sync.status]}: ${finished} of ${hosts.length} hosts done`} steps={steps} />
      <span className="muted">
        {finished} of {hosts.length}
      </span>
    </div>
  );
}
