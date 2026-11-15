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
  Disclosure,
  LogView,
  LogViewLine,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Skeleton,
  Spinner,
  StepList,
  type StepItem,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { getRouteApi, Link } from '@tanstack/react-router';
import { useState } from 'react';

import { approveSync, type Sync, type SyncHost, syncApprovalOptions, syncQuery } from '../api';
import { ErrorAlert, errorCode } from '../components/ErrorAlert';
import { PasskeyError } from '../components/PasskeyError';
import { formatClock, formatDateTime } from '../format';
import { signApproval } from '../webauthn';
import { byName } from './byPackage';
import { PlanByPackage } from './PlanByPackage';
import { finishedAt, planState, plural, tally } from './sync';

const route = getRouteApi('/sync/$syncId');

/** One sync as steps: the hosts, their plans, the signature, and each
 *  host's apply. A finished step keeps a short summary. */
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

/** Signs the plans of every host with changes, or only `hosts`. */
function useSign(sync: Sync) {
  const queryClient = useQueryClient();
  const toast = useToast();
  return useMutation({
    mutationFn: async (hosts?: string[]) => {
      const options = await syncApprovalOptions(sync.id, hosts);
      return approveSync(sync.id, await signApproval(options), hosts);
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
  // Done with no signature: every host is in sync, or no plan can be signed.
  const allInSync = nothing && inSync === states.length;
  const approval = sync.approvals[0];

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
      detail: signed || allInSync ? undefined : <PlanByPackage hosts={sync.hosts} />,
    },
    {
      key: 'sign',
      label: 'Sign',
      state: signed ? 'done' : nothing ? 'skipped' : sync.status === 'ready' ? 'running' : 'pending',
      meta: approval
        ? `${approval.by}, ${approval.passkey}, ${formatClock(approval.created_at)}`
        : nothing
          ? 'Nothing to sign'
          : undefined,
      detail:
        !signed && sync.status === 'ready' ? (
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
    {
      key: 'apply',
      label: 'Apply',
      state: !signed
        ? nothing
          ? 'skipped'
          : 'pending'
        : hosts.waiting.length > 0
          ? 'running'
          : hosts.failed.length > 0
            ? 'failed'
            : 'done',
      meta: signed
        ? [
            hosts.applied.length && `${hosts.applied.length} applied`,
            hosts.waiting.length && `${hosts.waiting.length} waiting`,
            hosts.failed.length && `${hosts.failed.length} failed`,
          ]
            .filter(Boolean)
            .join(', ')
        : undefined,
      detail: signed ? <p className="muted">Each host applies its own plan. Its steps are below.</p> : undefined,
    },
  ];

  // The hosts get their own StepList next to the run: a StepList nested in a
  // step takes that step's state for every mark.
  return (
    <div className="stack">
      <Finished sync={sync} nothing={nothing} allInSync={allInSync} />
      <Card>
        <CardContent>
          <StepList label="Sync progress" steps={steps} />
        </CardContent>
      </Card>
      {signed && (
        <section className="stack-sm">
          <h2 className="t-caps">Hosts</h2>
          <Card>
            <CardContent>
              <Apply sync={sync} sign={sign} />
            </CardContent>
          </Card>
        </section>
      )}
    </div>
  );
}

/** The end of a sync: every host reported. */
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

/** The hosts of a started sync in one line: "12 hosts: 8 Linux, 4 macOS,
 *  1 disconnected". "Show all" opens their names in natural order. */
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

function hostStep(host: SyncHost): StepItem['state'] {
  switch (host.step) {
    case 'applied':
      return 'done';
    case 'waiting':
      return 'running';
    case 'failed':
    case 'replan':
      return 'failed';
    case 'in_sync':
    case 'skipped':
      return 'skipped';
    default:
      return 'pending';
  }
}

const hostMeta: Record<SyncHost['step'], string> = {
  picked: 'Not signed',
  in_sync: 'In sync',
  skipped: 'Offline or no plan',
  waiting: 'Applying',
  applied: 'Applied',
  failed: 'Failed',
  replan: 'Plan changed, nothing ran',
};

function Apply({ sync, sign }: { sync: Sync; sign: ReturnType<typeof useSign> }) {
  const retrying = sign.isPending ? sign.variables?.[0] : undefined;
  const steps: StepItem[] = sync.hosts.map((host) => {
    const failed = host.step === 'failed' || host.step === 'replan';
    const at = host.result?.received_at ?? host.result?.reported_at;
    return {
      key: host.id,
      label: host.name,
      state: hostStep(host),
      meta: at && host.step !== 'waiting' ? `${hostMeta[host.step]}, ${formatClock(at)}` : hostMeta[host.step],
      detail: failed ? (
        <div className="stack-sm">
          {host.step === 'replan' && (
            <p>Its plan changed before it applied. Sign the new plan to apply it.</p>
          )}
          {host.result?.log && (
            <LogView className="apply-log" aria-label={`${host.name} apply log`}>
              {host.result.log.split('\n').map((line, index) => (
                <LogViewLine key={index}>{line}</LogViewLine>
              ))}
            </LogView>
          )}
          {sign.isError && sign.variables?.[0] === host.id && <PasskeyError error={sign.error} thing="sync" />}
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
        </div>
      ) : host.step === 'applied' && host.result?.log ? (
        <Disclosure summary="Apply log">
          <LogView className="apply-log" aria-label={`${host.name} apply log`}>
            {host.result.log.split('\n').map((line, index) => (
              <LogViewLine key={index}>{line}</LogViewLine>
            ))}
          </LogView>
        </Disclosure>
      ) : undefined,
    };
  });
  return (
    <div className="stack-sm">
      {sign.isError && errorCode(sign.error) === 'nothing_to_approve' && (
        <ErrorAlert error={sign.error} thing="sync" />
      )}
      <StepList label="Hosts" steps={steps} />
    </div>
  );
}
