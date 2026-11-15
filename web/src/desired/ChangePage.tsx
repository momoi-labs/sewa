import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
  Button,
  Card,
  CodeBlock,
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Disclosure,
  EmptyState,
  EmptyStateDescription,
  EmptyStateTitle,
  GridPane,
  Lifecycle,
  LogView,
  LogViewLine,
  PageHeader,
  PageHeaderTitle,
  PaneGrid,
  Skeleton,
  Spinner,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { getRouteApi, Link } from '@tanstack/react-router';
import { useState } from 'react';

import { type AffectedHost, type ChangeDetail, changeQuery, discardChange, hostsQuery, rolesQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { formatDateTime, formatReceived } from '../format';
import { HostStatusBadge } from '../hosts/HostStatusBadge';
import { ApplyResultBadge, applySummary } from './ApplyResult';
import { ApproveDialog } from './ApproveDialog';
import { ChangeStatusBadge } from './ChangeStatusBadge';
import { diffLines } from './compare';
import { describeEdit } from './describeEdit';
import { PlanView } from './PlanView';

const route = getRouteApi('/changes/$changeId');

/** One change: each affected host's plan side by side, and its edits. */
export function ChangePage() {
  const { changeId } = route.useParams();
  const change = useQuery(changeQuery(changeId));
  const [discarding, setDiscarding] = useState(false);
  const [approving, setApproving] = useState(false);
  const title = change.data
    ? change.data.revert_host
      ? `Revert of ${change.data.revert_host.name}`
      : `Change of ${formatDateTime(change.data.created_at)}`
    : null;
  const summary = change.data ? applySummary(change.data.hosts) : null;
  const replan = change.data?.hosts.some((host) => host.result?.status === 'replan') ?? false;
  const approvable =
    !!change.data &&
    ((change.data.status === 'draft' && change.data.hosts.length > 0) ||
      (change.data.status === 'approved' && replan));


  return (
    <>
      <Breadcrumb aria-label="Breadcrumb">
        <BreadcrumbList>
          <BreadcrumbItem>
            <BreadcrumbLink asChild>
              <Link to="/changes">Changes</Link>
            </BreadcrumbLink>
          </BreadcrumbItem>
          <BreadcrumbSeparator />
          <BreadcrumbItem>
            <BreadcrumbPage>{title ?? '…'}</BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </Breadcrumb>
      {change.isError && !change.data ? (
        <ErrorAlert error={change.error} thing="change" onRetry={() => void change.refetch()} />
      ) : (
        <>
          <div className="between">
            <PageHeader>
              <PageHeaderTitle>{title ?? <Skeleton />}</PageHeaderTitle>
            </PageHeader>
            {change.data && (
              <Lifecycle
                status={<ChangeStatusBadge status={change.data.status} />}
                actions={
                  approvable && (
                    <Button size="sm" onClick={() => setApproving(true)}>
                      {change.data.status === 'approved' ? 'Approve again' : 'Approve'}
                    </Button>
                  )
                }
                destructive={
                  change.data.status === 'draft' && (
                    <Button size="sm" variant="destructive" onClick={() => setDiscarding(true)}>
                      Discard
                    </Button>
                  )
                }
              />
            )}
          </div>
          {change.isError && <ErrorAlert error={change.error} thing="change" onRetry={() => void change.refetch()} />}
          {change.data && (
            <p className="muted">
              Opened by <span className="mono">{change.data.created_by}</span>.{' '}
              {change.data.status === 'draft'
                ? change.data.revert_host
                  ? 'Approving it applies the approved state on the host again, with the plan that fixes its drift.'
                  : 'Each host plans the draft. Nothing is applied until a change is approved.'
                : change.data.closed_at && `Closed ${formatDateTime(change.data.closed_at)}.`}
            </p>
          )}
          {summary && <p className="t-label">{summary}.</p>}
          <Card className="detail-tabs" data-fill="false">
            <Tabs defaultValue="plans" data-shadow-style="none">
              <TabsList aria-label="Change details">
                <TabsTrigger value="plans">Plans</TabsTrigger>
                <TabsTrigger value="edits">Edits</TabsTrigger>
                <TabsTrigger value="approvals">Approvals</TabsTrigger>
              </TabsList>
              <TabsContent value="plans">{change.data ? <Plans change={change.data} /> : <Skeleton />}</TabsContent>
              <TabsContent value="edits">{change.data ? <Edits change={change.data} /> : <Skeleton />}</TabsContent>
              <TabsContent value="approvals">
                {change.data ? <Approvals change={change.data} /> : <Skeleton />}
              </TabsContent>
            </Tabs>
          </Card>
          {change.data && <DiscardDialog change={change.data} open={discarding} onOpenChange={setDiscarding} />}
          {change.data && <ApproveDialog change={change.data} open={approving} onOpenChange={setApproving} />}
        </>
      )}
    </>
  );
}

function Plans({ change }: { change: ChangeDetail }) {
  if (change.hosts.length === 0) {
    return (
      <EmptyState variant="informational">
        <EmptyStateTitle>No host is affected</EmptyStateTitle>
        <EmptyStateDescription>
          {change.status === 'draft'
            ? 'The draft does not change the config of any host yet. Give a role to a host, or edit a role a host has.'
            : 'No host planned this change.'}
        </EmptyStateDescription>
      </EmptyState>
    );
  }
  return (
    <PaneGrid fill>
      {change.hosts.map((host) => (
        <GridPane key={host.id} id={host.id} title={host.name} size={6} min={4}>
          <HostPlan host={host} />
        </GridPane>
      ))}
    </PaneGrid>
  );
}

function HostPlan({ host }: { host: AffectedHost }) {
  const waiting =
    host.status === 'connected'
      ? 'The agent plans the new config on its next report.'
      : 'The host gets the config when its agent connects again.';
  return (
    <div className="stack">
      <div className="row-wrap">
        <HostStatusBadge status={host.status} />
        {host.result && <ApplyResultBadge status={host.result.status} />}
        <Link to="/hosts/$hostId" params={{ hostId: host.id }}>
          Open the host
        </Link>
      </div>
      {host.result && <HostResult result={host.result} />}
      <PlanView plan={host.plan} waiting={waiting} />
      {host.proposed_config !== null && (
        <Disclosure summary="Config change">
          <CodeBlock code={diffLines(host.current_config, host.proposed_config)} diff title="mise.toml" />
        </Disclosure>
      )}
    </div>
  );
}

/** What the host's agent said about the approval. */
function HostResult({ result }: { result: NonNullable<AffectedHost['result']> }) {
  const reported =
    (result.reported_at ? ` Reported ${formatDateTime(result.reported_at)}.` : '') +
    (result.received_at ? ` ${formatReceived(result.received_at)}` : '');
  const text = {
    waiting: 'The agent applies the plan on its next report.',
    applied: `The agent applied the plan with mise bootstrap.${reported}`,
    failed: `The agent did not apply the plan.${reported}`,
    replan: `The plan changed before the agent applied it, so nothing was applied. Approve the change again.${reported}`,
  }[result.status];
  return (
    <>
      <p className="muted">{text}</p>
      {result.log && result.status !== 'waiting' && (
        <Disclosure summary="Apply log" open={result.status === 'failed' || undefined}>
          <LogView className="apply-log" aria-label="Apply log">
            {result.log.split('\n').map((line, index) => (
              <LogViewLine key={index}>{line}</LogViewLine>
            ))}
          </LogView>
        </Disclosure>
      )}
    </>
  );
}

function Approvals({ change }: { change: ChangeDetail }) {
  if (change.approvals.length === 0) {
    return (
      <EmptyState variant="informational">
        <EmptyStateTitle>No approvals</EmptyStateTitle>
        <EmptyStateDescription>
          {change.status === 'draft'
            ? 'Approve the change to sign the plans with your passkey.'
            : 'Nobody approved this change.'}
        </EmptyStateDescription>
      </EmptyState>
    );
  }
  return (
    <TableFrame frame="none">
      <Table aria-label="Approvals">
        <TableHeader>
          <TableRow>
            <TableHead>Time</TableHead>
            <TableHead>By</TableHead>
            <TableHead>Passkey</TableHead>
            <TableHead className="num">Hosts</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {change.approvals.map((entry) => (
            <TableRow key={entry.id}>
              <TableCell>
                <time dateTime={entry.created_at}>{formatDateTime(entry.created_at)}</time>
              </TableCell>
              <TableCell className="mono">{entry.by}</TableCell>
              <TableCell>{entry.passkey}</TableCell>
              <TableCell className="num">{entry.host_count}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </TableFrame>
  );
}

function Edits({ change }: { change: ChangeDetail }) {
  const roles = useQuery(rolesQuery);
  const hosts = useQuery(hostsQuery);
  const names = new Map([...(roles.data ?? []), ...(hosts.data ?? [])].map((entry) => [entry.id, entry.name]));
  const name = (id: string) => names.get(id) ?? 'unknown';
  if (change.edits.length === 0) {
    return (
      <EmptyState variant="informational">
        <EmptyStateTitle>No edits</EmptyStateTitle>
        <EmptyStateDescription>This change has no edits.</EmptyStateDescription>
      </EmptyState>
    );
  }
  return (
    <TableFrame frame="none">
      <Table aria-label="Edits">
        <TableHeader>
          <TableRow>
            <TableHead>Time</TableHead>
            <TableHead>Edit</TableHead>
            <TableHead>By</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {change.edits.map((entry) => (
            <TableRow key={entry.id}>
              <TableCell>
                <time dateTime={entry.at}>{formatDateTime(entry.at)}</time>
              </TableCell>
              <TableCell>{describeEdit(entry.edit, name)}</TableCell>
              <TableCell className="mono">{entry.by}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </TableFrame>
  );
}

type DiscardProps = {
  change: ChangeDetail;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

/** Confirms before a draft is discarded. A discarded change cannot reopen. */
function DiscardDialog({ change, open, onOpenChange }: DiscardProps) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const mutation = useMutation({
    mutationFn: () => discardChange(change.id),
    onSuccess: async () => {
      await Promise.all(
        [['changes'], ['roles'], ['hosts']].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
      onOpenChange(false);
      toast('success', 'Draft discarded', 'No host will get its edits.');
    },
  });
  const edits = change.edit_count === 1 ? '1 edit' : `${change.edit_count} edits`;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) mutation.reset();
        onOpenChange(next);
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Discard the draft?</DialogTitle>
          <DialogDescription>
            Its {edits} are dropped and the hosts stop planning it. You cannot undo this.
          </DialogDescription>
        </DialogHeader>
        {mutation.isError && (
          <DialogBody>
            <ErrorAlert error={mutation.error} thing="change" />
          </DialogBody>
        )}
        <DialogFooter>
          <Button type="button" variant="ghost" autoFocus onClick={() => onOpenChange(false)}>
            Keep the draft
          </Button>
          <Button
            type="button"
            variant="destructive"
            aria-busy={mutation.isPending}
            onClick={() => {
              if (!mutation.isPending) mutation.mutate();
            }}
          >
            {mutation.isPending && <Spinner size="sm" />}
            Discard
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
