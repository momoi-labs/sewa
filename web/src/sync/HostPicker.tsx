import {
  Button,
  Checkbox,
  Search,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
} from '@momoi-labs/kiso-react';
import { ArrowDown, ArrowUp, ArrowUpDown } from 'lucide-react';
import { type ReactNode, useId, useState } from 'react';

import type { ListedHost } from '../api';
import { Unknown } from '../components/Unknown';
import { osNames } from '../format';
import { HostStatusBadge } from '../hosts/HostStatusBadge';
import { byName } from './byPackage';

type Filters = { os: Set<string>; status: Set<string>; role: Set<string> };
type Column = 'name' | 'os' | 'roles' | 'status';

const statuses = [
  ['connected', 'Connected'],
  ['disconnected', 'Disconnected'],
] as const;

/** A host's OS as a filter value: `unknown` until its agent reports it. */
const osOf = (host: ListedHost) => host.os ?? 'unknown';
const osLabels: Record<string, string> = { ...osNames, unknown: 'Unknown' };

const roleNames = (host: ListedHost) => host.roles.map((role) => role.name).join(', ');

const sortKeys: Record<Column, (host: ListedHost) => string> = {
  name: (host) => host.name,
  os: (host) => `${osLabels[osOf(host)]} ${host.name}`,
  roles: (host) => `${roleNames(host)} ${host.name}`,
  status: (host) => `${host.status === 'connected' ? '1' : '0'} ${host.name}`,
};

/** Hosts that pass the filters: a host matches a group when it has one of
 *  the group's values, and must match every group that has a value. */
function matches(host: ListedHost, filters: Filters) {
  return (
    (filters.os.size === 0 || filters.os.has(osOf(host))) &&
    (filters.status.size === 0 || filters.status.has(host.status)) &&
    (filters.role.size === 0 || host.roles.some((role) => filters.role.has(role.id)))
  );
}

/** The "Pick hosts" step: filters on the left, a search, quick buttons and
 *  a sortable table of hosts. The search, the filters and the buttons act
 *  on the hosts shown. */
export function HostPicker({
  hosts,
  chosen,
  onChange,
  action,
}: {
  hosts: ListedHost[];
  chosen: Set<string>;
  onChange: (chosen: Set<string>) => void;
  action: ReactNode;
}) {
  const [query, setQuery] = useState('');
  const [filters, setFilters] = useState<Filters>({ os: new Set(), status: new Set(), role: new Set() });
  const [sort, setSort] = useState<{ column: Column; direction: 1 | -1 }>({ column: 'name', direction: 1 });

  const typed = query.trim().toLowerCase();
  const key = sortKeys[sort.column];
  const shown = hosts
    .filter((host) => matches(host, filters) && host.name.toLowerCase().includes(typed))
    .sort((a, b) => byName(key(a), key(b)) * sort.direction);
  const picked = shown.filter((host) => chosen.has(host.id)).length;
  const header: boolean | 'indeterminate' = picked === 0 ? false : picked === shown.length ? true : 'indeterminate';

  const roles = new Map<string, { name: string; count: number }>();
  for (const host of hosts)
    for (const role of host.roles) roles.set(role.id, { name: role.name, count: (roles.get(role.id)?.count ?? 0) + 1 });
  const groups: { key: keyof Filters; label: string; values: { value: string; label: string; count: number }[] }[] = [
    {
      key: 'os',
      label: 'OS',
      values: Object.entries(osLabels)
        .map(([value, label]) => ({ value, label, count: hosts.filter((host) => osOf(host) === value).length }))
        .filter((entry) => entry.count > 0),
    },
    {
      key: 'status',
      label: 'Status',
      values: statuses.map(([value, label]) => ({
        value,
        label,
        count: hosts.filter((host) => host.status === value).length,
      })),
    },
    {
      key: 'role',
      label: 'Role',
      values: [...roles]
        .map(([value, { name, count }]) => ({ value, label: name, count }))
        .sort((a, b) => byName(a.label, b.label)),
    },
  ];

  const toggleFilter = (group: keyof Filters, value: string, on: boolean) => {
    const next = new Set(filters[group]);
    if (on) next.add(value);
    else next.delete(value);
    setFilters({ ...filters, [group]: next });
  };
  /** Changes the selection of the shown hosts only. */
  const pick = (select: (host: ListedHost) => boolean) => {
    const next = new Set(chosen);
    for (const host of shown) {
      if (select(host)) next.add(host.id);
      else next.delete(host.id);
    }
    onChange(next);
  };
  const sortBy = (column: Column) =>
    setSort(sort.column === column ? { column, direction: sort.direction === 1 ? -1 : 1 } : { column, direction: 1 });
  const heading = (column: Column, label: string) => {
    const on = sort.column === column;
    const Icon = !on ? ArrowUpDown : sort.direction === 1 ? ArrowUp : ArrowDown;
    return (
      <TableHead aria-sort={!on ? 'none' : sort.direction === 1 ? 'ascending' : 'descending'}>
        <Button type="button" variant="ghost" size="sm" className="sort-button" onClick={() => sortBy(column)}>
          {label}
          <Icon className="icon" aria-hidden="true" />
        </Button>
      </TableHead>
    );
  };
  const id = useId();

  return (
    <div className="stack-sm">
      <div className="host-picker">
        <div className="host-filters" role="group" aria-label="Filter hosts">
          {groups.map(
            (group) =>
              group.values.length > 0 && (
                <fieldset key={group.key} className="host-filter">
                  <legend className="t-caps">{group.label}</legend>
                  {group.values.map(({ value, label, count }) => (
                    <label key={value} className="host-filter-row">
                      <Checkbox
                        checked={filters[group.key].has(value)}
                        onCheckedChange={(checked) => toggleFilter(group.key, value, checked === true)}
                      />
                      <span>{label}</span>
                      <span className="muted mono">{count}</span>
                    </label>
                  ))}
                </fieldset>
              ),
          )}
        </div>
        <div className="stack-sm host-main">
          <Search
            aria-label="Filter hosts by name"
            placeholder="Filter by name"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <div className="row-wrap">
            <Button type="button" size="sm" onClick={() => pick(() => true)}>
              All
            </Button>
            <Button type="button" size="sm" onClick={() => pick((host) => host.os === 'linux')}>
              All Linux
            </Button>
            <Button type="button" size="sm" onClick={() => pick((host) => host.os === 'macos')}>
              All macOS
            </Button>
            <Button type="button" size="sm" onClick={() => pick(() => false)}>
              None
            </Button>
            <span className="muted host-count" role="status">
              {hosts.filter((host) => chosen.has(host.id)).length} of {hosts.length} selected
            </span>
          </div>
          {shown.length === 0 ? (
            <p className="muted" role="status">
              No host matches the search and the filters.
            </p>
          ) : (
            <TableFrame className="host-table" data-shadow-style="none">
              <Table density="compact" aria-label="Hosts to sync">
                <TableHeader>
                  <TableRow>
                    <TableHead className="host-check">
                      <Checkbox
                        aria-label="Select the shown hosts"
                        checked={header}
                        onCheckedChange={() => pick(() => header !== true)}
                      />
                    </TableHead>
                    {heading('name', 'Host')}
                    {heading('os', 'OS')}
                    {heading('roles', 'Roles')}
                    {heading('status', 'Status')}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {shown.map((host) => (
                    <TableRow key={host.id} data-state={chosen.has(host.id) ? 'selected' : undefined}>
                      <TableCell className="host-check">
                        <Checkbox
                          id={`${id}-${host.id}`}
                          aria-label={host.name}
                          checked={chosen.has(host.id)}
                          onCheckedChange={(checked) => {
                            const next = new Set(chosen);
                            if (checked === true) next.add(host.id);
                            else next.delete(host.id);
                            onChange(next);
                          }}
                        />
                      </TableCell>
                      <TableCell className="mono host-name">
                        <label htmlFor={`${id}-${host.id}`}>{host.name}</label>
                      </TableCell>
                      <TableCell>{host.os ? osNames[host.os] : <Unknown reason="The agent reports the OS when it connects" />}</TableCell>
                      <TableCell>{host.roles.length ? roleNames(host) : <span className="muted">None</span>}</TableCell>
                      <TableCell>
                        <HostStatusBadge status={host.status} />
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TableFrame>
          )}
          <div className="row-wrap">{action}</div>
        </div>
      </div>
    </div>
  );
}
