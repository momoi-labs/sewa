import {
  EmptyState,
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

import { changesQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { SkeletonRows } from '../components/SkeletonRows';
import { formatDateTime } from '../format';
import { ChangeStatusBadge } from './ChangeStatusBadge';

const COLUMNS = 4;

/** Every change, newest first. */
export function ChangesPage() {
  const changes = useQuery(changesQuery);


  return (
    <>
      <PageHeader>
        <PageHeaderTitle>Changes</PageHeaderTitle>
      </PageHeader>
      {changes.isError && <ErrorAlert error={changes.error} onRetry={() => void changes.refetch()} />}
      {changes.isSuccess && changes.data.length === 0 ? (
        <EmptyState variant="first-run">
          <EmptyStateTitle>No changes yet</EmptyStateTitle>
          <EmptyStateDescription>
            Edit a role or a host. The first edit opens a draft change, and each host plans it.
          </EmptyStateDescription>
        </EmptyState>
      ) : (
        !(changes.isError && !changes.data) && (
          <TableFrame>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="col-tight">Created</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>By</TableHead>
                  <TableHead className="num">Edits</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {changes.isPending ? (
                  <SkeletonRows columns={COLUMNS} />
                ) : (
                  changes.data?.map((change) => (
                    <TableRow key={change.id}>
                      <TableCell className="col-tight">
                        <Link to="/changes/$changeId" params={{ changeId: change.id }}>
                          {formatDateTime(change.created_at)}
                        </Link>
                      </TableCell>
                      <TableCell>
                        <ChangeStatusBadge status={change.status} />
                      </TableCell>
                      <TableCell className="mono">{change.created_by}</TableCell>
                      <TableCell className="num">{change.edit_count}</TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
            {changes.data && (
              <div className="table-footer">
                {changes.data.length === 1 ? '1 change' : `${changes.data.length} changes`}
              </div>
            )}
          </TableFrame>
        )
      )}
    </>
  );
}
