import {
  Alert,
  AlertContent,
  AlertDescription,
  AlertTitle,
  Badge,
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
  Button,
  Card,
  CardContent,
  Checkbox,
  Dot,
  LogView,
  LogViewLine,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Progress,
  RadioGroup,
  RadioGroupItem,
  Search,
  Skeleton,
  Spinner,
  StatusBadge,
  StepList,
  Steps,
  type StepItem,
  type StepsItem,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  useToast,
} from '@momoi-labs/kiso-react';
import { ArrowLeft, ChevronDown } from 'lucide-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { getRouteApi, Link } from '@tanstack/react-router';
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { flushSync } from 'react-dom';

import { ApiFailure, approveSync, type Sync, type SyncHost, syncApprovalOptions, syncQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { PasskeyError } from '../components/PasskeyError';
import { formatClock, formatDateTime, osNames } from '../format';
import { signApproval } from '../webauthn';
import { byName } from './byPackage';
import { PlanByPackage } from './PlanByPackage';
import {
  activity,
  type ActivityEvent,
  filterOf,
  finishedAt,
  type HostFilter,
  hostFilters,
  progressStepLabel,
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
      <SignedRun
        sync={sync}
        sign={sign}
        plans={plans}
        notice={<Finished sync={sync} nothing={nothing} allInSync={allInSync} />}
      />
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
  applying: 'Applying',
  applied: 'Applied',
  failed: 'Failed',
  replan: 'Plan changed',
};

function stepTone(step: SyncHost['step']): 'success' | 'danger' | 'neutral' {
  if (step === 'applied' || step === 'applying') return 'success';
  return step === 'failed' || step === 'replan' ? 'danger' : 'neutral';
}

function SignedRun({
  sync,
  sign,
  plans,
  notice,
}: {
  sync: Sync;
  sign: ReturnType<typeof useSign>;
  plans: number;
  notice: ReactNode;
}) {
  const narrow = useNarrow();
  const hosts = tally(sync);
  const [tab, setTab] = useState<'activity' | 'hosts'>('activity');
  const [fromActivity, setFromActivity] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<ReadonlySet<HostFilter>>(new Set());
  const [feedQuery, setFeedQuery] = useState('');
  const [hostQuery, setHostQuery] = useState('');
  const [order, setOrder] = useState<FeedOrder>('new');
  const [selected, setSelected] = useState<string | null>(null);
  const matches = (host: SyncHost) => {
    const filter = filterOf(host.step);
    return statuses.size === 0 || (filter !== null && statuses.has(filter));
  };
  const hostTyped = hostQuery.trim().toLowerCase();
  const visible = orderHosts(sync.hosts).filter((host) => matches(host) && host.name.toLowerCase().includes(hostTyped));
  const feedTyped = feedQuery.trim().toLowerCase();
  const events = activity(sync).filter((event) =>
    event.host ? matches(event.host) && event.host.name.toLowerCase().includes(feedTyped) : statuses.size === 0 && !feedTyped,
  );
  const current = visible.find((host) => host.id === selected) ?? visible[0];
  useEffect(() => {
    if (tab === 'hosts' && selected && (narrow || fromActivity)) {
      document.querySelector('.host-row[aria-current="true"]')?.scrollIntoView?.({ block: narrow ? 'start' : 'nearest' });
    }
  }, [narrow, selected, tab, fromActivity]);
  const counts: Record<HostFilter, number> = {
    failed: hosts.failed.length,
    applying: hosts.applying.length,
    waiting: hosts.waiting.length,
    applied: hosts.applied.length,
    in_sync: hosts.inSync.length,
    skipped: hosts.skipped.length,
  };
  const open = hosts.waiting.length + hosts.applying.length;
  const total = open + hosts.applied.length + hosts.failed.length;
  const finished = hosts.applied.length + hosts.failed.length;
  const approval = sync.approvals[0]!;
  const applyStatus: StepsItem['status'] = open > 0 ? undefined : hosts.failed.length > 0 ? 'error' : 'completed';
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
  const searched = tab === 'activity' ? feedTyped : hostTyped;
  const clear = () => {
    setStatuses(new Set());
    setFeedQuery('');
    setHostQuery('');
  };
  const openHost = (id: string, event: string) => {
    setSelected(id);
    setHostQuery('');
    setTab('hosts');
    setFromActivity(event);
  };
  const backToActivity = () => {
    const event = fromActivity;
    flushSync(() => {
      setTab('activity');
      setFromActivity(null);
    });
    [...document.querySelectorAll<HTMLElement>('.activity-row[data-key]')].find((row) => row.dataset.key === event)?.focus();
  };
  const detail = current && <HostDetail host={current} sign={sign} />;

  return (
    <Card className="run-panel">
      <CardContent className="stack-sm">
        {notice}
        <Steps label="Sync progress" items={steps} current={open > 0 ? 'apply' : null} />
        {total > 0 && (
          <Progress
            label="Apply progress"
            value={finished}
            max={total}
            valueText={`${finished} of ${plural(total, 'host')} finished`}
          />
        )}
      </CardContent>
      <Tabs
        className="run-tabs"
        value={tab}
        onValueChange={(value) => {
          setTab(value as typeof tab);
          setFromActivity(null);
        }}
      >
        <div className="run-tabbar">
          <TabsList aria-label="Sync run views">
            <TabsTrigger value="activity">
              Activity <Badge variant="neutral" className="badge-count">{events.length}</Badge>
            </TabsTrigger>
            <TabsTrigger value="hosts">
              Hosts <Badge variant="neutral" className="badge-count">{visible.length}</Badge>
            </TabsTrigger>
          </TabsList>
          <div className="list-filters run-tools">
            {tab === 'activity' && (
              <Search
                aria-label="Search hosts"
                placeholder="Search hosts"
                value={feedQuery}
                onChange={(event) => setFeedQuery(event.target.value)}
              />
            )}
            <StatusFilter value={statuses} counts={counts} narrow={narrow} onChange={setStatuses} />
            {tab === 'activity' && (
              <RadioGroup
                className="run-order"
                variant="segmented"
                label="Order"
                value={order}
                onValueChange={(value) => setOrder(value as FeedOrder)}
              >
                <RadioGroupItem value="new" label="Newest first" />
                <RadioGroupItem value="old" label="Oldest first" />
              </RadioGroup>
            )}
            <Button type="button" disabled={statuses.size === 0 && !searched} onClick={clear}>
              Clear filters
            </Button>
          </div>
        </div>
        <TabsContent value="activity">
          <Feed key={`${order} ${[...statuses].join()} ${feedTyped}`} events={events} order={order} onOpen={openHost} />
        </TabsContent>
        <TabsContent value="hosts">
          {fromActivity && (
            <div className="run-back">
              <Button type="button" size="sm" variant="ghost" autoFocus onClick={backToActivity}>
                <ArrowLeft className="icon" aria-hidden="true" />
                Back to activity
              </Button>
            </div>
          )}
          <div className="run-split">
            <div className="run-list stack-sm">
              <Search
                aria-label="Search hosts"
                placeholder="Search hosts"
                value={hostQuery}
                onChange={(event) => setHostQuery(event.target.value)}
              />
              {visible.length === 0 ? (
                <p className="muted" role="status">
                  No host matches.
                </p>
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
                        <Dot variant={stepTone(host.step)} pulse={host.step === 'applying'} aria-hidden="true" />
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
            </div>
            {!narrow && detail && <div className="run-detail">{detail}</div>}
          </div>
        </TabsContent>
      </Tabs>
    </Card>
  );
}

function statusLabel(value: ReadonlySet<HostFilter>, narrow: boolean) {
  if (value.size === 0) return 'Status';
  const names = `Status: ${hostFilters
    .filter((filter) => value.has(filter.key))
    .map((filter) => filter.label)
    .join(', ')}`;
  return names.length <= (narrow ? 18 : 26) ? names : `Status: ${value.size} selected`;
}

function StatusFilter({
  value,
  counts,
  narrow,
  onChange,
}: {
  value: ReadonlySet<HostFilter>;
  counts: Record<HostFilter, number>;
  narrow: boolean;
  onChange: (next: ReadonlySet<HostFilter>) => void;
}) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button type="button" className="status-filter">
          <span className="truncate">{statusLabel(value, narrow)}</span>
          {value.size > 0 && (
            <Badge variant="neutral" className="badge-count" aria-hidden="true">
              {value.size}
            </Badge>
          )}
          <ChevronDown className="icon" aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end">
        <fieldset className="host-filter">
          <legend className="t-caps">Status</legend>
          {hostFilters.map((filter) => (
            <label key={filter.key} className="host-filter-row">
              <Checkbox
                checked={value.has(filter.key)}
                onCheckedChange={(checked) => {
                  const next = new Set(value);
                  if (checked === true) next.add(filter.key);
                  else next.delete(filter.key);
                  onChange(next);
                }}
              />
              <span>{filter.label}</span>
              <span className="muted mono">{counts[filter.key]}</span>
            </label>
          ))}
        </fieldset>
      </PopoverContent>
    </Popover>
  );
}

type FeedOrder = 'new' | 'old';

function Feed({ events, order, onOpen }: { events: ActivityEvent[]; order: FeedOrder; onOpen: (host: string, event: string) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [away, setAway] = useState(false);
  const shown = order === 'new' ? events : [...events].reverse();
  const keys = shown.map((event) => event.key).join(' ');
  const [seen, setSeen] = useState(keys);
  if (!away && seen !== keys) setSeen(keys);
  useLayoutEffect(() => {
    const feed = ref.current;
    if (order === 'old' && !away && feed) feed.scrollTop = feed.scrollHeight;
  }, [order, away, keys]);
  const known = new Set(seen.split(' '));
  const fresh = away ? shown.filter((event) => !known.has(event.key)).length : 0;
  const jump = () => {
    const feed = ref.current;
    if (feed) feed.scrollTop = order === 'new' ? 0 : feed.scrollHeight;
    setAway(false);
  };
  const pill = fresh > 0 && (
    <div className={`feed-new feed-new-${order === 'new' ? 'top' : 'bottom'}`}>
      <Button type="button" size="sm" variant="primary" onClick={jump}>
        {order === 'new' ? `${fresh} new` : `${fresh} new below`}
      </Button>
    </div>
  );

  if (events.length === 0) {
    return (
      <p className="muted run-feed-empty" role="status">
        No event matches.
      </p>
    );
  }
  return (
    <div
      ref={ref}
      className="run-feed"
      onScroll={() => {
        const feed = ref.current;
        if (!feed) return;
        const next = order === 'new' ? feed.scrollTop > 8 : feed.scrollHeight - feed.scrollTop - feed.clientHeight > 8;
        if (next !== away) setAway(next);
      }}
    >
      {order === 'new' && pill}
      <ol className="activity-feed" aria-label="Activity">
        {shown.map((event) => {
          const content = (
            <>
              <time className="mono muted" dateTime={event.at}>
                {formatClock(event.at)}
              </time>
              <span className="activity-who">
                <Dot
                  variant={event.kind === 'failed' ? 'danger' : event.kind === 'applied' ? 'success' : 'neutral'}
                  aria-hidden="true"
                />
                <strong className="truncate">{event.who}</strong>
              </span>
              <span className="activity-text">{event.what}</span>
            </>
          );
          const host = event.host;
          return (
            <li key={event.key}>
              {host ? (
                <button type="button" className="activity-row" data-key={event.key} onClick={() => onOpen(host.id, event.key)}>
                  {content}
                </button>
              ) : (
                <div className="activity-row">{content}</div>
              )}
            </li>
          );
        })}
      </ol>
      {order === 'old' && pill}
    </div>
  );
}

function logLines(log: string) {
  return log.split('\n').map((line, index) => <LogViewLine key={index}>{line}</LogViewLine>);
}

const SIGNED: SyncHost['step'][] = ['waiting', 'applying', 'applied', 'failed', 'replan'];

function applySteps(host: SyncHost): StepItem[] {
  const done = host.step === 'applied';
  const ended = done || host.step === 'failed' || host.step === 'replan';
  const at = host.result?.reported_at ?? host.result?.received_at;
  const ran = host.progress?.steps ?? [];
  const steps: StepItem[] = [{ key: 'signed', label: 'Signed', state: 'done' }];
  if (host.progress) {
    steps.push({ key: 'checked', label: 'Checked', state: 'done' });
    ran.forEach((step, index) => {
      const last = index === ran.length - 1;
      steps.push({
        key: step.step,
        label: step.step === 'native' ? 'Native packages' : step.step,
        state: last && !ended ? 'running' : 'done',
        meta: formatClock(step.at),
      });
    });
    if (!ran.some((step) => step.step === 'mise')) {
      steps.push({ key: 'mise', label: 'mise', state: 'pending', meta: ended ? 'Not run' : undefined });
    }
  }
  steps.push({
    key: 'reported',
    label: 'Reported',
    state: done ? 'done' : ended ? 'failed' : 'pending',
    meta: ended && at ? formatClock(at) : undefined,
  });
  return steps;
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
        <StatusBadge tone={stepTone(host.step)} pulse={host.step === 'applying'}>
          {stepLabel[host.step]}
        </StatusBadge>
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
      {host.step === 'applying' &&
        (host.status === 'connected' ? (
          <p className="muted">
            Applying: {progressStepLabel[host.progress?.step ?? ''] ?? host.progress?.step}
            {host.progress && `, started ${formatClock(host.progress.started_at)}`}
          </p>
        ) : (
          <Alert variant="warning">
            <AlertContent>
              <AlertDescription>
                No report since {host.last_seen_at ? formatClock(host.last_seen_at) : 'it started'}. The agent may have
                stopped; its result arrives when it reports again.
              </AlertDescription>
            </AlertContent>
          </Alert>
        ))}
      {SIGNED.includes(host.step) && <StepList label={`${host.name} apply steps`} steps={applySteps(host)} />}
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
