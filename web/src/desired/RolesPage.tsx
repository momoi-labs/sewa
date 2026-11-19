import {
  Badge,
  Button,
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
import { countOf, ListEmpty, ListFilters, NoMatchRow } from '../components/ListFilters';
import { SkeletonRows } from '../components/SkeletonRows';

const COLUMNS = 4;

export function roleOs(role: Role): string {
  const kinds = new Set(role.items.map((item) => item.os ?? null));
  if (kinds.size === 1 && kinds.has('linux')) return 'Linux';
  if (kinds.size === 1 && kinds.has('macos')) return 'macOS';
  return 'Linux and macOS';
}

export function RolesPage() {
  const roles = useQuery(rolesQuery);
  const [query, setQuery] = useState('');
  const typed = query.trim().toLowerCase();
  const shown = (roles.data ?? []).filter((role) => role.name.toLowerCase().includes(typed));
  const add = (
    <Button size="sm" variant="primary" asChild>
      <Link to="/roles/new">
        <Plus className="icon" aria-hidden="true" />
        Add a role
      </Link>
    </Button>
  );

  return (
    <>
      <PageHeader actions={add}>
        <PageHeaderTitle>Roles</PageHeaderTitle>
      </PageHeader>
      {roles.isError && <ErrorAlert error={roles.error} onRetry={() => void roles.refetch()} />}
      {roles.isSuccess && roles.data.length === 0 ? (
        <ListEmpty title="No roles yet">Add a role, give it packages and mise tools, then give it to hosts.</ListEmpty>
      ) : (
        !(roles.isError && !roles.data) && (
          <>
            <ListFilters
              thing="roles"
              query={query}
              onQuery={setQuery}
              filtering={typed !== ''}
              onClear={() => setQuery('')}
            />
          <TableFrame data-fill="false">
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
                ) : shown.length === 0 ? (
                  <NoMatchRow thing="roles" columns={COLUMNS} />
                ) : (
                  shown.map((role) => (
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
              <div className="table-footer">{countOf(shown.length, roles.data.length, 'roles')}</div>
            )}
          </TableFrame>
          </>
        )
      )}
    </>
  );
}
