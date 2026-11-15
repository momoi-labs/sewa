import type { QueryClient } from '@tanstack/react-query';
import { useQueryErrorResetBoundary } from '@tanstack/react-query';
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  type ErrorComponentProps,
  type RouterHistory,
} from '@tanstack/react-router';

import { ErrorPage } from './components/ErrorPage';
import { NotFound } from './components/NotFound';
import { Shell } from './components/Shell';
import { HostPage } from './hosts/HostPage';
import { HostsPage } from './hosts/HostsPage';
import { ChangePage } from './desired/ChangePage';
import { ChangesPage } from './desired/ChangesPage';
import { RolePage } from './desired/RolePage';
import { RolesPage } from './desired/RolesPage';
import { PasskeysPage } from './passkeys/PasskeysPage';

/** Takes the place of the shell when a page's first load fails with
 *  `not_admin`, a 5xx, or no answer. Retrying loads the page again. */
function RootError({ error, reset }: ErrorComponentProps) {
  const queryReset = useQueryErrorResetBoundary();
  return (
    <ErrorPage
      error={error}
      onRetry={() => {
        queryReset.reset();
        reset();
      }}
    />
  );
}

const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  component: Shell,
  errorComponent: RootError,
  notFoundComponent: NotFound,
});

const hostsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/', component: HostsPage });

const hostRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/hosts/$hostId',
  component: HostPage,
});

const rolesRoute = createRoute({ getParentRoute: () => rootRoute, path: '/roles', component: RolesPage });

const roleRoute = createRoute({ getParentRoute: () => rootRoute, path: '/roles/$roleId', component: RolePage });

const changesRoute = createRoute({ getParentRoute: () => rootRoute, path: '/changes', component: ChangesPage });

const changeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/changes/$changeId',
  component: ChangePage,
});

const passkeysRoute = createRoute({ getParentRoute: () => rootRoute, path: '/passkeys', component: PasskeysPage });

const routeTree = rootRoute.addChildren([
  hostsRoute,
  hostRoute,
  rolesRoute,
  roleRoute,
  changesRoute,
  changeRoute,
  passkeysRoute,
]);

export function createAppRouter(queryClient: QueryClient, history?: RouterHistory) {
  return createRouter({ routeTree, context: { queryClient }, history });
}

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
