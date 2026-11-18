import { ApplicationShell, type ApplicationShellGroup, ThemeSelector } from '@momoi-labs/kiso-react';
import { Link, Outlet, useNavigate, useRouterState } from '@tanstack/react-router';
import { KeyRound, Layers, RefreshCw, Server } from 'lucide-react';
import { useState } from 'react';

import { HeaderBreadcrumb } from './HeaderBreadcrumb';
import { BrandSymbol, Lockup } from './Logo';
import { applyTheme, savedTheme, type Theme } from './theme';

export function Shell() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [theme, setTheme] = useState<Theme>(savedTheme);
  const site = window.location.host;

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
      collapsible
      togglePlacement="header"
      shellLayout
      brandMark={<BrandSymbol />}
      brand={
        <Link to="/" className="console-brand" aria-label="sewa, home">
          <span className="console-brand-full">
            <Lockup />
            <span className="console-dns">{site}</span>
          </span>
          <span className="console-brand-compact">
            <BrandSymbol />
            <span className="console-dns">{site}</span>
          </span>
        </Link>
      }
      navigation={navigation}
      navigationLabel="Main"
      header={
        <>
          <HeaderBreadcrumb site={site} />
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
