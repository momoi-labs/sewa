import { ApplicationShell, type ApplicationShellGroup, ThemeSelector } from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { Link, Outlet, useNavigate, useRouterState } from '@tanstack/react-router';
import { KeyRound, Layers, RefreshCw, Server } from 'lucide-react';
import { useState } from 'react';

import { instanceQuery } from '../api';
import { SewaMark } from './SewaMark';
import { applyTheme, savedTheme, type Theme } from './theme';

/** The frame of every page: the brand, the theme, and the destinations
 *  Hosts, Roles, Sync and Passkeys. The brand slot shows
 *  the instance address under the name. */
export function Shell() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [theme, setTheme] = useState<Theme>(savedTheme);
  const instance = useQuery({ ...instanceQuery, throwOnError: false });

  const navigation: ApplicationShellGroup[] = [
    {
      destinations: [
        {
          href: '/',
          label: 'Hosts',
          leading: <Server className="icon" aria-hidden="true" />,
          active: pathname === '/' || pathname.startsWith('/hosts/'),
          onClick: () => void navigate({ to: '/' }),
        },
        {
          href: '/roles',
          label: 'Roles',
          leading: <Layers className="icon" aria-hidden="true" />,
          active: pathname.startsWith('/roles'),
          onClick: () => void navigate({ to: '/roles' }),
        },
        {
          href: '/sync',
          label: 'Sync',
          leading: <RefreshCw className="icon" aria-hidden="true" />,
          active: pathname.startsWith('/sync'),
          onClick: () => void navigate({ to: '/sync' }),
        },
        {
          href: '/passkeys',
          label: 'Passkeys',
          leading: <KeyRound className="icon" aria-hidden="true" />,
          active: pathname.startsWith('/passkeys'),
          onClick: () => void navigate({ to: '/passkeys' }),
        },
      ],
    },
  ];

  return (
    <ApplicationShell
      brand={
        <Link to="/" className="brand brand-slot" aria-label="sewa, home">
          <SewaMark />
          <span className="brand-slot-text">
            <span className="brand-slot-name">sewa</span>
            {instance.data && <span className="brand-slot-site">{address(instance.data.address)}</span>}
          </span>
        </Link>
      }
      navigation={navigation}
      navigationLabel="Main"
      header={
        <>
          <div className="grow" />
          <ThemeSelector
            theme={theme}
            onChange={(next) => {
              const chosen = next as Theme;
              setTheme(chosen);
              applyTheme(chosen);
            }}
          />
        </>
      }
    >
      <div className="page">
        <Outlet />
      </div>
    </ApplicationShell>
  );
}

/** The instance address without its scheme, as the header shows it. */
function address(url: string): string {
  return url.replace(/^https?:\/\//, '').replace(/\/$/, '');
}
