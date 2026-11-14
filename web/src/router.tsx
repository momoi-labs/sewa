import type { QueryClient } from '@tanstack/react-query';
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  type RouterHistory,
} from '@tanstack/react-router';

import { NotFound } from './components/NotFound';
import { Shell } from './components/Shell';
import { HostPage } from './hosts/HostPage';
import { HostsPage } from './hosts/HostsPage';

const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  component: Shell,
  notFoundComponent: NotFound,
});

const hostsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/', component: HostsPage });

const hostRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/hosts/$hostId',
  component: HostPage,
});

const routeTree = rootRoute.addChildren([hostsRoute, hostRoute]);

export function createAppRouter(queryClient: QueryClient, history?: RouterHistory) {
  return createRouter({ routeTree, context: { queryClient }, history });
}

declare module '@tanstack/react-router' {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
