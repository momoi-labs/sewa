import {
  Alert,
  AlertContent,
  AlertDescription,
  AlertTitle,
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
  Button,
  Card,
  CardContent,
  Dot,
  LogView,
  LogViewLine,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Progress,
  Search,
  Skeleton,
  Spinner,
  StatusBadge,
  StepList,
  Steps,
  type StepItem,
  type StepsItem,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { getRouteApi, Link } from '@tanstack/react-router';
import { useEffect, useState } from 'react';

import { ApiFailure, approveSync, type Sync, type SyncHost, syncApprovalOptions, syncQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { PasskeyError } from '../components/PasskeyError';
import { formatClock, formatDateTime, osNames } from '../format';
import { signApproval } from '../webauthn';
import { byName } from './byPackage';
import { PlanByPackage } from './PlanByPackage';
import {
  activity,
  filterOf,
  finishedAt,
  type HostFilter,
  hostFilters,
  orderHosts,
  planState,
  plural,
  sameTargets,
  tally,
} from './sync';

const route = getRouteApi('/sync/$syncId');

export function SyncRunPage() {
  const { syncId } = route.useParams();
  const sync = useQuery(syncQuery(syncId));

  return (
    <>
      <Breadcrumb aria-label="Breadcrumb">
        <BreadcrumbList>
          <BreadcrumbItem>
            <BreadcrumbLink asChild>
              <Link to="/sync">Sync</Link>
            </BreadcrumbLink>
          </BreadcrumbItem>
          <BreadcrumbSeparator />
          <BreadcrumbItem>
            <BreadcrumbPage>{sync.data ? formatDateTime(sync.data.created_at) : '…'}</BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </Breadcrumb>
      <PageHeader>
        <PageHeaderTitle>Sync</PageHeaderTitle>
        <PageHeaderDescription>
          {sync.data
            ? `Started ${formatDateTime(sync.data.created_at)} by ${sync.data.created_by}.`
            : 'Each host plans, you sign the plans, and each host applies its own.'}
        </PageHeaderDescription>
      </PageHeader>
      {sync.isError && !sync.data ? (
        <ErrorAlert error={sync.error} thing="sync" onRetry={() => void sync.refetch()} />
      ) : sync.data ? (
        <Run sync={sync.data} />
      ) : (
        <Skeleton />
      )}
    </>
  );
}

function useSign(sync: Sync) {
  const queryClient = useQueryClient();
  const toast = useToast();
  return useMutation({
    mutationFn: async (hosts?: string[]) => {
      const options = await syncApprovalOptions(sync.id, hosts);
      if (!sameTargets(sync, options.hosts, hosts)) throw new ApiFailure(409, 'stale_plans');
      return approveSync(sync.id, options.approval_id, await signApproval(options), hosts);
    },
    onSuccess: async (next) => {
      queryClient.setQueryData(['syncs', next.id], next);
      await Promise.all([['syncs'], ['hosts']].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
      toast('success', 'Plans signed', 'Each agent checks the signature, then applies its plan.');
    },
  });
}

function Run({ sync }: { sync: Sync }) {
  const sign = useSign(sync);
  const hosts = tally(sync);
  const signed = sync.approvals.length > 0;
  const toSign = hosts.picked.filter((host) => planState(host) === 'changes');
  const nothing = !signed && sync.status === 'done';
  const states = sync.hosts.map((host) => planState(host));
  const plans = states.filter((state) => state === 'changes').length;
  const inSync = states.filter((state) => state === 'in_sync').length;
  const offline = states.filter((state) => state === 'offline').length;
  const failedPlans = states.filter((state) => state === 'failed').length;
  const allInSync = nothing && inSync === states.length;

  if (signed) {
    return (
      <div className="stack">
        <Finished sync={sync} nothing={nothing} allInSync={allInSync} />
        <SignedRun sync={sync} sign={sign} plans={plans} />
      </div>
    );
  }

  const steps: StepItem[] = [
    {
      key: 'hosts',
      label: 'Pick hosts',
      state: 'done',
      meta: plural(sync.hosts.length, 'host'),
      detail: <PickedHosts hosts={sync.hosts} />,
    },
    {
      key: 'plans',
      label: 'Plans',
      state: sync.status === 'planning' ? 'running' : 'done',
      meta: [
        plans && plural(plans, 'plan'),
        inSync && `${inSync} in sync`,
        offline && `${offline} offline`,
        failedPlans && `${failedPlans} failed`,
      ]
        .filter(Boolean)
        .join(', '),
      detail: allInSync ? undefined : <PlanByPackage hosts={sync.hosts} />,
    },
    {
      key: 'sign',
      label: 'Sign',
      state: nothing ? 'skipped' : sync.status === 'ready' ? 'running' : 'pending',
      meta: nothing ? 'Nothing to sign' : undefined,
      detail:
        sync.status === 'ready' ? (
          <div className="stack-sm">
            <p>
              Your passkey signs the {plural(toSign.length, 'plan')} above. Each agent checks the signature and
              applies its own plan. A host whose plan changes first does not apply.
            </p>
            {sign.isError && <PasskeyError error={sign.error} thing="sync" />}
            <div className="row-wrap">
              <Button
                type="button"
                variant="primary"
                aria-busy={sign.isPending}
                onClick={() => {
                  if (!sign.isPending) sign.mutate(undefined);
                }}
              >
                {sign.isPending && <Spinner size="sm" />}
                Sign and sync
              </Button>
            </div>
          </div>
        ) : undefined,
    },
    { key: 'apply', label: 'Apply', state: nothing ? 'skipped' : 'pending' },
  ];

  return (
    <div className="stack">
      <Finished sync={sync} nothing={nothing} allInSync={allInSync} />
      <Card>
        <CardContent>
          <StepList label="Sync progress" steps={steps} />
        </CardContent>
      </Card>
    </div>
  );
}

function useNarrow() {
  const query = '(max-width: 719px)';
  const [narrow, setNarrow] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setNarrow(media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);
  return narrow;
}

const stepLabel: Record<SyncHost['step'], string> = {
  picked: 'Not signed',
  in_sync: 'In sync',
  skipped: 'Skipped',
  waiting: 'Waiting',
  applied: 'Applied',
  failed: 'Failed',
  replan: 'Plan changed',
};

function stepTone(step: SyncHost['step']): 'success' | 'danger' | 'neutral' {
  if (step === 'applied') return 'success';
  return step === 'failed' || step === 'replan' ? 'danger' : 'neutral';
}

function SignedRun({ sync, sign, plans }: { sync: Sync; sign: ReturnType<typeof useSign>; plans: number }) {
  const narrow = useNarrow();
  const hosts = tally(sync);
  const [query, setQuery] = useState('');
  const [filters, setFilters] = useState<ReadonlySet<HostFilter>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  const typed = query.trim().toLowerCase();
  const visible = orderHosts(sync.hosts).filter((host) => {
    const filter = filterOf(host.step);
    return (
      host.name.toLowerCase().includes(typed) && (filters.size === 0 || (filter !== null && filters.has(filter)))
    );
  });
  const current = visible.find((host) => host.id === selected) ?? visible[0];
  useEffect(() => {
    if (narrow && selected) {
      document.querySelector('.host-row[aria-current="true"]')?.scrollIntoView?.({ block: 'start' });
    }
  }, [narrow, selected]);
  const counts: Record<HostFilter, number> = {
    failed: hosts.failed.length,
    waiting: hosts.waiting.length,
    applied: hosts.applied.length,
    in_sync: hosts.inSync.length,
    skipped: hosts.skipped.length,
  };
  const total = hosts.waiting.length + hosts.applied.length + hosts.failed.length;
  const finished = hosts.applied.length + hosts.failed.length;
  const approval = sync.approvals[0]!;
  const applyStatus: StepsItem['status'] = hosts.waiting.length > 0 ? undefined : hosts.failed.length > 0 ? 'error' : 'completed';
  const steps: StepsItem[] = [
    { id: 'hosts', label: 'Pick hosts', status: 'completed', description: plural(sync.hosts.length, 'host') },
    { id: 'plans', label: 'Plans', status: 'completed', description: plural(plans, 'plan') },
    { id: 'sign', label: 'Sign', status: 'completed', description: formatClock(approval.created_at) },
    {
      id: 'apply',
      label: 'Apply',
      status: applyStatus,
      description: hosts.failed.length > 0 ? `${hosts.failed.length} failed` : undefined,
    },
  ];
  const toggle = (key: HostFilter) => {
    const next = new Set(filters);
    if (!next.delete(key)) next.add(key);
    setFilters(next);
  };
  const detail = current && <HostDetail host={current} sign={sign} />;

  return (
    <>
      <Card>
        <CardContent className="stack-sm">
          <Steps
            label="Sync progress"
            items={steps}
            current={hosts.waiting.length > 0 ? 'apply' : null}
          />
          {total > 0 && (
            <Progress
              label="Apply progress"
              value={finished}
              max={total}
              valueText={`${finished} of ${plural(total, 'host')} finished`}
            />
          )}
          <div className="run-chips" role="group" aria-label="Filter hosts by status">
            {hostFilters.map((filter) => (
              <Button
                key={filter.key}
                type="button"
                size="sm"
                variant={filters.has(filter.key) ? 'primary' : 'default'}
                aria-pressed={filters.has(filter.key)}
                onClick={() => toggle(filter.key)}
              >
                {filter.label} <span className="mono">{counts[filter.key]}</span>
              </Button>
            ))}
          </div>
        </CardContent>
      </Card>
      <div className="run-split">
        <Card>
          <CardContent className="stack-sm">
            <Search
              aria-label="Search hosts"
              placeholder="Search hosts"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            {visible.length === 0 ? (
              <div className="stack-sm" role="status">
                <p className="muted">No host matches.</p>
                <div className="row-wrap">
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => {
                      setQuery('');
                      setFilters(new Set());
                    }}
                  >
                    Clear filters
                  </Button>
                </div>
              </div>
            ) : (
              <ul className="host-list" aria-label="Hosts">
                {visible.map((host) => (
                  <li key={host.id}>
                    <button
                      type="button"
                      className="host-row"
                      aria-current={host.id === current?.id ? 'true' : undefined}
                      onClick={() => setSelected(host.id)}
                    >
                      <Dot variant={stepTone(host.step)} aria-hidden="true" />
                      <span className="host-row-name truncate" title={host.name}>
                        {host.name}
                      </span>
                      <span className="host-row-state">{stepLabel[host.step]}</span>
                    </button>
                    {narrow && host.id === current?.id && detail}
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
        {!narrow && detail && (
          <Card>
            <CardContent>{detail}</CardContent>
          </Card>
        )}
      </div>
      <ActivityFeed sync={sync} />
    </>
  );
}

function logLines(log: string) {
  return log.split('\n').map((line, index) => <LogViewLine key={index}>{line}</LogViewLine>);
}

function HostDetail({ host, sign }: { host: SyncHost; sign: ReturnType<typeof useSign> }) {
  const retrying = sign.isPending ? sign.variables?.[0] : undefined;
  const failed = host.step === 'failed' || host.step === 'replan';
  const at = host.result?.received_at ?? host.result?.reported_at;
  const log = host.result?.log;
  const showPlan = (failed || host.step === 'waiting') && planState(host) === 'changes';
  const when = [host.os && osNames[host.os], at && formatClock(at)].filter(Boolean).join(', ');

  return (
    <section className="host-detail stack-sm" aria-label={`${host.name} detail`}>
      <div className="between host-detail-head">
        <div className="host-detail-title">
          <h2 className="host-detail-name">{host.name}</h2>
          {when && <p className="muted">{when}</p>}
        </div>
        <StatusBadge tone={stepTone(host.step)}>{stepLabel[host.step]}</StatusBadge>
      </div>
      {host.step === 'replan' && (
        <Alert variant="warning">
          <AlertContent>
            <AlertTitle>Plan changed, nothing ran</AlertTitle>
            <AlertDescription>Its plan changed before it applied. Retry signs the new plan below.</AlertDescription>
          </AlertContent>
        </Alert>
      )}
      {host.step === 'waiting' && (
        <p className="muted">
          {host.status === 'connected'
            ? 'A waiting host picks up the change on its next check-in, within 30 seconds.'
            : host.status === 'disabled'
              ? 'This host is disabled, so it does not apply the change. Enable it to continue.'
              : `This host is ${host.status === 'disconnected' ? 'offline' : 'not connected yet'}, so nothing happens yet. It applies the change on its first check-in after it connects.`}
        </p>
      )}
      {host.step === 'skipped' && (
        <p className="muted">Offline or no plan when the sync was signed, so nothing ran. It catches up when you sync it again.</p>
      )}
      {host.step === 'in_sync' && <p className="muted">Its plan has nothing to change.</p>}
      {showPlan && (
        <section className="stack-sm">
          <h3 className="t-caps">Plan</h3>
          <PlanByPackage key={host.id} hosts={[host]} />
        </section>
      )}
      {log && (
        <section className="stack-sm">
          <h3 className="t-caps">Log</h3>
          {log.startsWith('…') && <p className="muted">The controller keeps only the end of a long log.</p>}
          <LogView className="apply-log" aria-label={`${host.name} apply log`}>
            {logLines(log)}
          </LogView>
        </section>
      )}
      {sign.isError && sign.variables?.[0] === host.id && <PasskeyError error={sign.error} thing="sync" />}
      {failed && (
        <div className="row-wrap">
          <Button
            type="button"
            size="sm"
            aria-busy={retrying === host.id}
            disabled={sign.isPending && retrying !== host.id}
            onClick={() => {
              if (!sign.isPending) sign.mutate([host.id]);
            }}
          >
            {retrying === host.id && <Spinner size="sm" />}
            Retry {host.name}
          </Button>
        </div>
      )}
    </section>
  );
}

function ActivityFeed({ sync }: { sync: Sync }) {
  const events = activity(sync);
  return (
    <Card>
      <CardContent className="stack-sm">
        <h2 className="t-caps">Activity</h2>
        <ol className="activity-feed" aria-label="Activity">
          {events.map((event) => (
            <li key={event.key}>
              <time className="mono muted" dateTime={event.at}>
                {formatClock(event.at)}
              </time>
              <Dot variant={event.kind === 'failed' ? 'danger' : 'success'} aria-hidden="true" />
              <span className="activity-text">
                <strong>{event.who}</strong> {event.what}
              </span>
            </li>
          ))}
        </ol>
      </CardContent>
    </Card>
  );
}

function Finished({ sync, nothing, allInSync }: { sync: Sync; nothing: boolean; allInSync: boolean }) {
  if (sync.status !== 'done') return null;
  const hosts = tally(sync);
  if (nothing && !allInSync) {
    const states = sync.hosts.map((host) => planState(host));
    const failed = states.filter((state) => state === 'failed').length;
    const offline = states.filter((state) => state === 'offline').length;
    const parts = [
      failed && `${plural(failed, 'plan')} failed`,
      offline && `${plural(offline, 'host')} offline`,
    ].filter(Boolean);
    return (
      <Alert variant="warning">
        <AlertContent>
          <AlertTitle>Nothing can be signed</AlertTitle>
          <AlertDescription>
            No host has a plan to sign: {parts.join(', ')}. Fix the plans or bring the hosts online, then start
            a new sync.
          </AlertDescription>
        </AlertContent>
      </Alert>
    );
  }
  if (nothing) {
    return (
      <Alert variant="success">
        <AlertContent>
          <AlertTitle>Nothing to sync</AlertTitle>
          <AlertDescription>
            Every host you picked planned the desired state and has nothing to change.
          </AlertDescription>
        </AlertContent>
      </Alert>
    );
  }
  const parts = [
    hosts.applied.length && `${plural(hosts.applied.length, 'host')} applied`,
    hosts.failed.length && `${hosts.failed.length} failed`,
    hosts.skipped.length && `${hosts.skipped.length} skipped`,
    hosts.inSync.length && `${hosts.inSync.length} already in sync`,
  ].filter(Boolean);
  const failed = hosts.failed.length > 0;
  return (
    <Alert variant={failed ? 'error' : 'success'}>
      <AlertContent>
        <AlertTitle>
          {failed ? 'Sync finished with failures' : 'Sync finished'} at {formatClock(finishedAt(sync))}
        </AlertTitle>
        <AlertDescription>
          {parts.join(', ')}.{' '}
          {failed
            ? 'Open a failed host below to read its log, then retry it.'
            : hosts.skipped.length
              ? 'A skipped host catches up when you sync it again.'
              : 'The hosts match the desired state.'}
        </AlertDescription>
      </AlertContent>
    </Alert>
  );
}

function PickedHosts({ hosts }: { hosts: SyncHost[] }) {
  const [open, setOpen] = useState(false);
  const count = (keep: (host: SyncHost) => boolean) => hosts.filter(keep).length;
  const parts = [
    [count((host) => host.os === 'linux'), 'Linux'],
    [count((host) => host.os === 'macos'), 'macOS'],
    [count((host) => host.status === 'disconnected'), 'disconnected'],
  ]
    .filter(([n]) => n !== 0)
    .map(([n, label]) => `${n} ${label}`);
  return (
    <div className="stack-sm">
      <p>
        {plural(hosts.length, 'host')}
        {parts.length > 0 && `: ${parts.join(', ')}`}.{' '}
        <Button
          type="button"
          variant="ghost"
          className="btn-link"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          {open ? 'Hide' : 'Show all'}
        </Button>
      </p>
      {open && <p className="muted">{hosts.map((host) => host.name).sort(byName).join(', ')}.</p>}
    </div>
  );
}
