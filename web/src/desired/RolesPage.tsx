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
import { SkeletonRows } from '../components/SkeletonRows';
import { AddRoleDialog } from './AddRoleDialog';

const COLUMNS = 4;

/** The OS a role's items go to: one OS when every item names it. */
export function roleOs(role: Role): string {
  const kinds = new Set(role.items.map((item) => item.os ?? null));
  if (kinds.size === 1 && kinds.has('linux')) return 'Linux';
  if (kinds.size === 1 && kinds.has('macos')) return 'macOS';
  return 'Linux and macOS';
}

/** Every role, the built-in role first, with its OS and how many items and
 *  hosts it has. */
export function RolesPage() {
  const roles = useQuery(rolesQuery);
  const [adding, setAdding] = useState(false);
  const add = (
    <Button size="sm" variant="primary" onClick={() => setAdding(true)}>
      <Plus className="icon" aria-hidden="true" />
      Add a role
    </Button>
  );

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
                  <TableHead>Name</TableHead>
                  <TableHead>OS</TableHead>
                  <TableHead className="num">Items</TableHead>
                  <TableHead className="num">Hosts</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {roles.isPending ? (
                  <SkeletonRows columns={COLUMNS} />
                ) : (
                  roles.data?.map((role) => (
                    <TableRow key={role.id}>
                      <TableCell>
                        <div className="row-wrap">
                          <Link to="/roles/$roleId" params={{ roleId: role.id }}>
                            {role.name}
                          </Link>
                          {role.builtin && <Badge variant="neutral">Built in</Badge>}
                        </div>
                      </TableCell>
                      <TableCell>{roleOs(role)}</TableCell>
                      <TableCell className="num">{role.items.length}</TableCell>
                      <TableCell className="num">{role.builtin ? 'All' : role.hosts.length}</TableCell>
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
