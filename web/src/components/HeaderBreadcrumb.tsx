import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { useNavigate, useRouterState } from '@tanstack/react-router';
import type { MouseEvent } from 'react';

import { hostQuery, roleQuery, syncQuery } from '../api';
import { formatDateTime } from '../format';

const AREAS: Record<string, { label: string; to: string }> = {
  hosts: { label: 'Hosts', to: '/' },
  roles: { label: 'Roles', to: '/roles' },
  sync: { label: 'Sync', to: '/sync' },
  passkeys: { label: 'Passkeys', to: '/passkeys' },
};

export function HeaderBreadcrumb({ site }: { site: string }) {
  const navigate = useNavigate();
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  });
  const [first, id] = pathname.split('/').filter(Boolean);
  const area = AREAS[first ?? 'hosts'];
  const detail = first === 'hosts' || first === 'roles' || first === 'sync' ? id : undefined;
  const host = useQuery({
    ...hostQuery(id ?? ''),
    enabled: first === 'hosts' && !!id,
    throwOnError: false,
  });
  const role = useQuery({
    ...roleQuery(id ?? ''),
    enabled: first === 'roles' && !!id,
    throwOnError: false,
  });
  const run = useQuery({
    ...syncQuery(id ?? ''),
    enabled: first === 'sync' && !!id,
    throwOnError: false,
  });

  if (!area) return null;

  const go = (to: string) => (event: MouseEvent) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    void navigate({ to });
  };

  const name =
    first === 'hosts'
      ? host.data?.name
      : first === 'roles'
        ? role.data?.name
        : run.data && formatDateTime(run.data.created_at);

  return (
    <Breadcrumb aria-label="Breadcrumb">
      <BreadcrumbList>
        <BreadcrumbItem>
          <BreadcrumbLink href="/" className="console-dns" onClick={go('/')}>
            {site}
          </BreadcrumbLink>
        </BreadcrumbItem>
        <BreadcrumbSeparator />
        {detail ? (
          <>
            <BreadcrumbItem>
              <BreadcrumbLink href={area.to} onClick={go(area.to)}>
                {area.label}
              </BreadcrumbLink>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
            <BreadcrumbItem>
              <BreadcrumbPage>{name ?? '…'}</BreadcrumbPage>
            </BreadcrumbItem>
          </>
        ) : (
          <BreadcrumbItem>
            <BreadcrumbPage>{area.label}</BreadcrumbPage>
          </BreadcrumbItem>
        )}
      </BreadcrumbList>
    </Breadcrumb>
  );
}
