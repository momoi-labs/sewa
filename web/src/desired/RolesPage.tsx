import {
  Badge,
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

import { type Role, rolesQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { ErrorPage, isPageError } from '../components/ErrorPage';
import { SkeletonRows } from '../components/SkeletonRows';
import { AddRoleDialog } from './AddRoleDialog';
import { compareItems, compareNamed } from './compare';

const COLUMNS = 4;

function drafted(role: Role): boolean {
  return (
    compareItems(role.items, role.proposed_items).some((row) => row.difference !== 'same') ||
    compareNamed(role.hosts, role.proposed_hosts).some((row) => row.difference !== 'same')
  );
}

/** Every role, with how many items and hosts it has. */
export function RolesPage() {
  const roles = useQuery(rolesQuery);
  const [adding, setAdding] = useState(false);
  const add = (
    <Button size="sm" variant="primary" onClick={() => setAdding(true)}>
      <Plus className="icon" aria-hidden="true" />
      Add a role
    </Button>
  );

  if (roles.isError && !roles.data && isPageError(roles.error)) {
    return <ErrorPage error={roles.error} onRetry={() => void roles.refetch()} />;
  }

  return (
    <>
      <PageHeader actions={add}>
        <PageHeaderTitle>Roles</PageHeaderTitle>
      </PageHeader>
      {roles.isError && <ErrorAlert error={roles.error} onRetry={() => void roles.refetch()} />}
      {roles.isSuccess && roles.data.length === 0 ? (
        <EmptyState variant="first-run">
          <EmptyStateTitle>No roles yet</EmptyStateTitle>
          <EmptyStateDescription>
            Add a role, give it packages and mise tools, then give it to hosts.
          </EmptyStateDescription>
          <EmptyStateActions>{add}</EmptyStateActions>
        </EmptyState>
      ) : (
        !(roles.isError && !roles.data) && (
          <TableFrame>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="col-tight">Name</TableHead>
                  <TableHead className="num">Items</TableHead>
                  <TableHead className="num">Hosts</TableHead>
                  <TableHead>Draft</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {roles.isPending ? (
                  <SkeletonRows columns={COLUMNS} />
                ) : (
                  roles.data?.map((role) => (
                    <TableRow key={role.id}>
                      <TableCell className="col-tight">
                        <Link to="/roles/$roleId" params={{ roleId: role.id }}>
                          {role.name}
                        </Link>
                      </TableCell>
                      <TableCell className="num">{role.items.length}</TableCell>
                      <TableCell className="num">{role.hosts.length}</TableCell>
                      <TableCell>{drafted(role) ? <Badge variant="warning">Edited in draft</Badge> : null}</TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
            {roles.data && (
              <div className="table-footer">{roles.data.length === 1 ? '1 role' : `${roles.data.length} roles`}</div>
            )}
          </TableFrame>
        )
      )}
      <AddRoleDialog open={adding} onOpenChange={setAdding} />
    </>
  );
}
