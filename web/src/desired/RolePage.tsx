import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
  Button,
  Card,
  EmptyState,
  EmptyStateDescription,
  EmptyStateTitle,
  PageHeader,
  PageHeaderTitle,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Skeleton,
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
} from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { getRouteApi, Link } from '@tanstack/react-router';
import { useState } from 'react';

import { hostsQuery, type Role, roleQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { compareItems, compareNamed } from './compare';
import { DifferenceBadge } from './DifferenceBadge';
import { DraftNotice } from './DraftNotice';
import { ItemForm } from './ItemForm';
import { ItemsTable } from './ItemsTable';
import { useEdit } from './useEdit';

const route = getRouteApi('/roles/$roleId');

/** One role: its items and its hosts. Every edit goes into the draft change. */
export function RolePage() {
  const { roleId } = route.useParams();
  const role = useQuery(roleQuery(roleId));

  return (
    <>
      <Breadcrumb aria-label="Breadcrumb">
        <BreadcrumbList>
          <BreadcrumbItem>
            <BreadcrumbLink asChild>
              <Link to="/roles">Roles</Link>
            </BreadcrumbLink>
          </BreadcrumbItem>
          <BreadcrumbSeparator />
          <BreadcrumbItem>
            <BreadcrumbPage>{role.data?.name ?? '…'}</BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </Breadcrumb>
      {role.isError && !role.data ? (
        <ErrorAlert error={role.error} thing="role" onRetry={() => void role.refetch()} />
      ) : (
        <>
          <PageHeader>
            <PageHeaderTitle>{role.data?.name ?? <Skeleton />}</PageHeaderTitle>
          </PageHeader>
          <DraftNotice />
          {role.isError && <ErrorAlert error={role.error} thing="role" onRetry={() => void role.refetch()} />}
          <Card className="detail-tabs" data-fill="false">
            <Tabs defaultValue="items" data-shadow-style="none">
              <TabsList aria-label="Role details">
                <TabsTrigger value="items">Items</TabsTrigger>
                <TabsTrigger value="hosts">Hosts</TabsTrigger>
              </TabsList>
              <TabsContent value="items">{role.data ? <Items role={role.data} /> : <Skeleton />}</TabsContent>
              <TabsContent value="hosts">{role.data ? <Hosts role={role.data} /> : <Skeleton />}</TabsContent>
            </Tabs>
          </Card>
        </>
      )}
    </>
  );
}

function Items({ role }: { role: Role }) {
  const edit = useEdit();
  const rows = compareItems(role.items, role.proposed_items);
  return (
    <div className="stack">
      {edit.isError && <ErrorAlert error={edit.error} thing="role" />}
      {rows.length === 0 ? (
        <EmptyState variant="first-run">
          <EmptyStateTitle>No items yet</EmptyStateTitle>
          <EmptyStateDescription>Add a package or a mise tool. It goes into the draft change.</EmptyStateDescription>
        </EmptyState>
      ) : (
        <ItemsTable
          rows={rows}
          label={`Items of ${role.name}`}
          onRemove={(row) =>
            edit.mutate({
              op: 'role_item_remove',
              role: role.id,
              kind: row.kind,
              name: row.name,
            })
          }
        />
      )}
      <h3 className="t-h3">Add an item</h3>
      <ItemForm
        submitLabel="Add to draft"
        onAdd={(item) => edit.mutateAsync({ op: 'role_item_set', role: role.id, ...item })}
      />
    </div>
  );
}

function Hosts({ role }: { role: Role }) {
  const edit = useEdit();
  const hosts = useQuery(hostsQuery);
  const [chosen, setChosen] = useState('');
  const rows = compareNamed(role.hosts, role.proposed_hosts);
  const have = new Set(rows.filter((row) => row.difference !== 'removed').map((row) => row.id));
  const others = (hosts.data ?? []).filter((host) => !have.has(host.id));

  return (
    <div className="stack">
      {edit.isError && <ErrorAlert error={edit.error} thing="role" />}
      {hosts.isError && <ErrorAlert error={hosts.error} onRetry={() => void hosts.refetch()} />}
      {rows.length === 0 ? (
        <EmptyState variant="informational">
          <EmptyStateTitle>No hosts have this role</EmptyStateTitle>
          <EmptyStateDescription>Give it to a host below. That goes into the draft change.</EmptyStateDescription>
        </EmptyState>
      ) : (
        <TableFrame frame="none">
          <Table aria-label={`Hosts of ${role.name}`}>
            <TableHeader>
              <TableRow>
                <TableHead>Host</TableHead>
                <TableHead>Draft</TableHead>
                <TableHead aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.id}>
                  <TableCell>
                    <Link to="/hosts/$hostId" params={{ hostId: row.id }}>
                      {row.name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <DifferenceBadge difference={row.difference} />
                  </TableCell>
                  <TableCell>
                    {row.difference !== 'removed' && (
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`Take the role from ${row.name}`}
                        onClick={() =>
                          edit.mutate({
                            op: 'host_role_remove',
                            host: row.id,
                            role: role.id,
                          })
                        }
                      >
                        Take away
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
      {others.length > 0 && (
        <form
          className="row-wrap"
          onSubmit={(event) => {
            event.preventDefault();
            if (!chosen) return;
            edit.mutate({ op: 'host_role_add', host: chosen, role: role.id });
            setChosen('');
          }}
        >
          <Select value={chosen} onValueChange={setChosen}>
            <SelectTrigger aria-label="Host to give the role to">
              <SelectValue placeholder="Choose a host" />
            </SelectTrigger>
            <SelectContent>
              {others.map((host) => (
                <SelectItem key={host.id} value={host.id}>
                  {host.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button type="submit" disabled={!chosen}>
            Give to host in draft
          </Button>
        </form>
      )}
    </div>
  );
}
