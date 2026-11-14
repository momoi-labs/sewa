import { ApplicationShell, type ApplicationShellGroup, ThemeSelector } from '@momoi-labs/kiso-react';
import { Link, Outlet, useNavigate, useRouterState } from '@tanstack/react-router';
import { GitPullRequestDraft, KeyRound, Layers, Server } from 'lucide-react';
import { useState } from 'react';

import { SewaMark } from './SewaMark';
import { applyTheme, savedTheme, type Theme } from './theme';

/** The frame of every page: the brand, the theme, and the destinations
 *  Hosts, Roles, Changes and Passkeys. */
export function Shell() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [theme, setTheme] = useState<Theme>(savedTheme);

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
          href: '/changes',
          label: 'Changes',
          leading: <GitPullRequestDraft className="icon" aria-hidden="true" />,
          active: pathname.startsWith('/changes'),
          onClick: () => void navigate({ to: '/changes' }),
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
        <Link to="/" className="brand">
          <SewaMark />
          <span className="t-label">sewa</span>
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
