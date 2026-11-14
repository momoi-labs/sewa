import { ApplicationShell, type ApplicationShellGroup, ThemeSelector } from '@momoi-labs/kiso-react';
import { Link, Outlet, useNavigate, useRouterState } from '@tanstack/react-router';
import { Server } from 'lucide-react';
import { useState } from 'react';

import { applyTheme, savedTheme, type Theme } from './theme';

/** The frame of every page: the brand, the theme, and one destination, Hosts. */
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
      ],
    },
  ];

  return (
    <ApplicationShell
      brand={
        <Link to="/" className="brand">
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
