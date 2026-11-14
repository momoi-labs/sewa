import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
  Card,
  EmptyState,
  EmptyStateDescription,
  EmptyStateTitle,
  KV,
  KVKey,
  KVValue,
  Lifecycle,
  PageHeader,
  PageHeaderTitle,
  Skeleton,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { getRouteApi, Link } from '@tanstack/react-router';
import type { ReactNode } from 'react';

import { eventsQuery, type Host, type HostEvent, hostQuery, hostStateQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { SkeletonRows } from '../components/SkeletonRows';
import { Unknown } from '../components/Unknown';
import { HostDesired, HostDrift } from '../desired/HostDesired';
import { formatAgo, formatDateTime, formatReceived, osNames } from '../format';
import { HostStatusBadge } from './HostStatusBadge';

const route = getRouteApi('/hosts/$hostId');

/** One host: what its agent reports, its desired state, its drift, and its
 *  events. */
export function HostPage() {
  const { hostId } = route.useParams();
  const host = useQuery(hostQuery(hostId));
  const events = useQuery(eventsQuery(hostId));
  const state = useQuery(hostStateQuery(hostId));

  return (
    <>
      <Breadcrumb aria-label="Breadcrumb">
        <BreadcrumbList>
          <BreadcrumbItem>
            <BreadcrumbLink asChild>
              <Link to="/">Hosts</Link>
            </BreadcrumbLink>
          </BreadcrumbItem>
          <BreadcrumbSeparator />
          <BreadcrumbItem>
            <BreadcrumbPage>{host.data?.name ?? '…'}</BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </Breadcrumb>
      {host.isError && !host.data ? (
        <ErrorAlert error={host.error} onRetry={() => void host.refetch()} />
      ) : (
        <>
          <div className="between">
            <PageHeader>
              <PageHeaderTitle>{host.data?.name ?? <Skeleton />}</PageHeaderTitle>
            </PageHeader>
            {host.data && <Lifecycle status={<HostStatusBadge status={host.data.status} />} />}
          </div>
          {host.isError && <ErrorAlert error={host.error} onRetry={() => void host.refetch()} />}
          <Card className="detail-tabs">
            <Tabs defaultValue="summary" data-shadow-style="none">
              <TabsList aria-label="Host details">
                <TabsTrigger value="summary">Summary</TabsTrigger>
                <TabsTrigger value="desired">Desired state</TabsTrigger>
                <TabsTrigger value="drift">Drift</TabsTrigger>
                <TabsTrigger value="events">Events</TabsTrigger>
              </TabsList>
              <TabsContent value="summary">
                {host.data ? <Summary host={host.data} now={host.dataUpdatedAt} /> : <Skeleton />}
              </TabsContent>
              <TabsContent value="desired">
                {state.isError && !state.data ? (
                  <ErrorAlert error={state.error} onRetry={() => void state.refetch()} />
                ) : host.data && state.data ? (
                  <HostDesired host={host.data} state={state.data} />
                ) : (
                  <Skeleton />
                )}
              </TabsContent>
              <TabsContent value="drift">
                {state.isError && !state.data ? (
                  <ErrorAlert error={state.error} onRetry={() => void state.refetch()} />
                ) : state.data ? (
                  host.data ? <HostDrift host={host.data} state={state.data} /> : <Skeleton />
                ) : (
                  <Skeleton />
                )}
              </TabsContent>
              <TabsContent value="events" className="detail-pane">
                <Events
                  events={events.data}
                  error={events.isError ? events.error : null}
                  onRetry={() => void events.refetch()}
                />
              </TabsContent>
            </Tabs>
          </Card>
        </>
      )}
    </>
  );
}

function Moment({ at, now }: { at: string; now?: number }) {
  return (
    <time dateTime={at} title={now === undefined ? undefined : formatDateTime(at)}>
      {now === undefined ? formatDateTime(at) : formatAgo(at, now)}
    </time>
  );
}

function Summary({ host, now }: { host: Host; now: number }) {
  const notYet = (reason: string) => <Unknown reason={reason} />;
  const agent = (value: string | null | undefined): ReactNode =>
    value ?? notYet('The agent has not reported this yet');
  return (
    <KV>
      <KVKey>OS</KVKey>
      <KVValue>{osNames[host.os] ?? host.os}</KVValue>
      <KVKey>Created</KVKey>
      <KVValue>
        <Moment at={host.created_at} />
      </KVValue>
      <KVKey>Enrolled</KVKey>
      <KVValue>{host.enrolled_at ? <Moment at={host.enrolled_at} /> : notYet('The host has not enrolled yet')}</KVValue>
      <KVKey>Last seen</KVKey>
      <KVValue>
        {host.last_seen_at ? <Moment at={host.last_seen_at} now={now} /> : notYet('The agent has not reported yet')}
      </KVValue>
      <KVKey>Agent version</KVKey>
      <KVValue className="mono">{agent(host.agent?.version)}</KVValue>
      <KVKey>Hostname</KVKey>
      <KVValue className="mono">{agent(host.agent?.hostname)}</KVValue>
      <KVKey>Architecture</KVKey>
      <KVValue className="mono">{agent(host.agent?.arch)}</KVValue>
      <KVKey>Agent user</KVKey>
      <KVValue className="mono">{agent(host.agent?.user)}</KVValue>
    </KV>
  );
}

/** One sentence for each kind of event. */
function by(event: HostEvent): string {
  return typeof event.data.by === 'string' ? event.data.by : 'Someone';
}

export function describeEvent(event: HostEvent): string {
  switch (event.kind) {
    case 'host.created': {
      const os = typeof event.data.os === 'string' ? (osNames[event.data.os] ?? event.data.os) : null;
      return os ? `Host created for ${os}` : 'Host created';
    }
    case 'host.enrolled':
      return 'Install token traded for an agent credential';
    case 'host.connected':
      return 'Agent connected';
    case 'host.disconnected':
      return 'Agent stopped reporting';
    case 'host.offline':
      return typeof event.data.from === 'string' && typeof event.data.until === 'string'
        ? `No reports from ${formatDateTime(event.data.from)} to ${formatDateTime(event.data.until)}`
        : 'No reports for a while';
    case 'change.edited':
      return 'A draft change edits this host';
    case 'drift.detected':
      return 'The host drifted from its approved state';
    case 'change.approved':
      return `${by(event)} approved a change for this host`;
    case 'plan.applied':
      return 'The agent applied an approved change';
    case 'apply.failed':
      return 'The agent did not apply an approved change';
    case 'apply.replan':
      return 'The plan changed before the apply, so the change needs a new approval';
    case 'drift.reverted':
      return `${by(event)} opened a change to revert the drift`;
    case 'drift.adopted':
      return typeof event.data.package === 'string'
        ? `${by(event)} adopted ${event.data.package} in the draft`
        : `${by(event)} adopted a package in the draft`;
    case 'drift.ignored':
      return `${by(event)} ignored the drift`;
    default:
      return event.kind;
  }
}

function Events({
  events,
  error,
  onRetry,
}: {
  events: HostEvent[] | undefined;
  error: unknown;
  onRetry: () => void;
}) {
  if (error && !events) return <ErrorAlert error={error} onRetry={onRetry} />;
  if (events?.length === 0) {
    return (
      <EmptyState variant="informational">
        <EmptyStateTitle>No events</EmptyStateTitle>
        <EmptyStateDescription>Nothing has happened to this host yet.</EmptyStateDescription>
      </EmptyState>
    );
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Time</TableHead>
          <TableHead>Event</TableHead>
          <TableHead>Kind</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {events ? (
          events.map((event) => (
            <TableRow key={event.id}>
              <TableCell>
                <Moment at={event.at} />
              </TableCell>
              <TableCell>
                {describeEvent(event)}
                {typeof event.data.received_at === 'string' && (
                  <div className="muted">{formatReceived(event.data.received_at)}</div>
                )}
              </TableCell>
              <TableCell className="mono muted">{event.kind}</TableCell>
            </TableRow>
          ))
        ) : (
          <SkeletonRows columns={3} />
        )}
      </TableBody>
    </Table>
  );
}
