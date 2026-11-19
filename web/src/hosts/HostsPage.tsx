import {
  Button,
  PageHeader,
  PageHeaderTitle,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
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

import { type HostStatus, hostsQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { countOf, ListEmpty, ListFilters, NoMatchRow } from '../components/ListFilters';
import { SkeletonRows } from '../components/SkeletonRows';
import { Unknown } from '../components/Unknown';
import { formatAgo, formatDateTime, osNames } from '../format';
import { AddHostDialog } from './AddHostDialog';
import { HostStatusBadge, hostStatusLabels } from './HostStatusBadge';

const COLUMNS = 5;

export function HostsPage() {
  const hosts = useQuery(hostsQuery);
  const [adding, setAdding] = useState(false);
  const [query, setQuery] = useState('');
  const [status, setStatus] = useState<HostStatus | 'all'>('all');
  const typed = query.trim().toLowerCase();
  const filtering = typed !== '' || status !== 'all';
  const shown = (hosts.data ?? []).filter(
    (host) => host.name.toLowerCase().includes(typed) && (status === 'all' || host.status === status),
  );
  const now = hosts.dataUpdatedAt;
  const add = (
    <Button size="sm" variant="primary" onClick={() => setAdding(true)}>
      <Plus className="icon" aria-hidden="true" />
      Add a host
    </Button>
  );

  return (
    <>
      <PageHeader actions={add}>
        <PageHeaderTitle>Hosts</PageHeaderTitle>
      </PageHeader>
      {hosts.isError && <ErrorAlert error={hosts.error} onRetry={() => void hosts.refetch()} />}
      {hosts.isSuccess && hosts.data.length === 0 ? (
        <ListEmpty title="No hosts yet">Add a host to get the command that installs the agent on it.</ListEmpty>
      ) : (
        !(hosts.isError && !hosts.data) && (
          <>
            <ListFilters
              thing="hosts"
              query={query}
              onQuery={setQuery}
              filtering={filtering}
              onClear={() => {
                setQuery('');
                setStatus('all');
              }}
            >
              <Select value={status} onValueChange={(value) => setStatus(value as HostStatus | 'all')}>
                <SelectTrigger aria-label="Status">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">All statuses</SelectItem>
                  {(Object.keys(hostStatusLabels) as HostStatus[]).map((value) => (
                    <SelectItem key={value} value={value}>
                      {hostStatusLabels[value]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </ListFilters>
          <TableFrame data-fill="false">
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
                ) : shown.length === 0 ? (
                  <NoMatchRow thing="hosts" columns={COLUMNS} />
                ) : (
                  shown.map((host) => (
                    <TableRow key={host.id}>
                      <TableCell className="col-tight">
                        <Link to="/hosts/$hostId" params={{ hostId: host.id }}>
                          {host.name}
                        </Link>
                      </TableCell>
                      <TableCell>{host.os ? osNames[host.os] : <Unknown reason="The agent reports the OS when it connects" />}</TableCell>
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
              <div className="table-footer">{countOf(shown.length, hosts.data.length, 'hosts')}</div>
            )}
          </TableFrame>
          </>
        )
      )}
      <AddHostDialog open={adding} onOpenChange={setAdding} />
    </>
  );
}
