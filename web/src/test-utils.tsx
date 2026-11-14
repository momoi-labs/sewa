import { Toasts } from '@momoi-labs/kiso-react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, RouterProvider } from '@tanstack/react-router';
import { render } from '@testing-library/react';

import { createAppRouter } from './router';

/** Renders the app at `path`, with a fresh router and query cache. */
export function renderRoute(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, refetchInterval: false } } });
  const router = createAppRouter(queryClient, createMemoryHistory({ initialEntries: [path] }));
  return render(
    <QueryClientProvider client={queryClient}>
      <Toasts>
        <RouterProvider router={router} />
      </Toasts>
    </QueryClientProvider>,
  );
}

type Reply = { status: number; body?: unknown };

/** Replaces fetch with canned replies by "METHOD /path". Unknown calls get a 404. */
export function mockApi(replies: Record<string, Reply | ((body: unknown) => Reply)>) {
  const calls: { method: string; path: string; body: unknown }[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    const path = typeof input === 'string' ? input : input.toString();
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({ method, path, body });
    const entry = replies[`${method} ${path}`];
    const reply = typeof entry === 'function' ? entry(body) : (entry ?? { status: 404, body: { error: 'not_found' } });
    return new Response(JSON.stringify(reply.body ?? null), {
      status: reply.status,
      headers: { 'Content-Type': 'application/json' },
    });
  }) as typeof fetch;
  return calls;
}
