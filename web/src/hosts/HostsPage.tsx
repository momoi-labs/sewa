import {
  Button,
  EmptyState,
  EmptyStateActions,
  EmptyStateDescription,
  EmptyStateTitle,
  PageHeader,
  PageHeaderTitle,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
} from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Plus } from 'lucide-react';
import { useState } from 'react';

import { hostsQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { ErrorPage, isPageError } from '../components/ErrorPage';
import { SkeletonRows } from '../components/SkeletonRows';
import { Unknown } from '../components/Unknown';
import { formatAgo, formatDateTime, osNames } from '../format';
import { AddHostDialog } from './AddHostDialog';
import { HostStatusBadge } from './HostStatusBadge';

const COLUMNS = 5;

/** Every host with its status, read again every few seconds. */
export function HostsPage() {
  const hosts = useQuery(hostsQuery);
  const [adding, setAdding] = useState(false);
  const now = hosts.dataUpdatedAt;
  const add = (
    <Button size="sm" variant="primary" onClick={() => setAdding(true)}>
      <Plus className="icon" aria-hidden="true" />
      Add a host
    </Button>
  );

  if (hosts.isError && !hosts.data && isPageError(hosts.error)) {
    return <ErrorPage error={hosts.error} onRetry={() => void hosts.refetch()} />;
  }

  return (
    <>
      <PageHeader actions={add}>
        <PageHeaderTitle>Hosts</PageHeaderTitle>
      </PageHeader>
      {hosts.isError && <ErrorAlert error={hosts.error} onRetry={() => void hosts.refetch()} />}
      {hosts.isSuccess && hosts.data.length === 0 ? (
        <EmptyState variant="first-run">
          <EmptyStateTitle>No hosts yet</EmptyStateTitle>
          <EmptyStateDescription>
            Add a host to get the command that installs the agent on it.
          </EmptyStateDescription>
          <EmptyStateActions>{add}</EmptyStateActions>
        </EmptyState>
      ) : (
        !(hosts.isError && !hosts.data) && (
          <TableFrame>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="col-tight">Name</TableHead>
                  <TableHead>OS</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Agent version</TableHead>
                  <TableHead>Last seen</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {hosts.isPending ? (
                  <SkeletonRows columns={COLUMNS} />
                ) : (
                  hosts.data?.map((host) => (
                    <TableRow key={host.id}>
                      <TableCell className="col-tight">
                        <Link to="/hosts/$hostId" params={{ hostId: host.id }}>
                          {host.name}
                        </Link>
                      </TableCell>
                      <TableCell>{osNames[host.os] ?? host.os}</TableCell>
                      <TableCell>
                        <HostStatusBadge status={host.status} />
                      </TableCell>
                      <TableCell className="mono">
                        {host.agent?.version ?? <Unknown reason="No agent has reported yet" />}
                      </TableCell>
                      <TableCell>
                        {host.last_seen_at ? (
                          <time dateTime={host.last_seen_at} title={formatDateTime(host.last_seen_at)}>
                            {formatAgo(host.last_seen_at, now)}
                          </time>
                        ) : (
                          <Unknown reason="The agent has not reported yet" />
                        )}
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
            {hosts.data && (
              <div className="table-footer">
                {hosts.data.length === 1 ? '1 host' : `${hosts.data.length} hosts`}
              </div>
            )}
          </TableFrame>
        )
      )}
      <AddHostDialog open={adding} onOpenChange={setAdding} />
    </>
  );
}
