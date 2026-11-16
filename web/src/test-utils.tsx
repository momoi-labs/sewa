import { Toasts } from '@momoi-labs/kiso-react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, RouterProvider } from '@tanstack/react-router';
import { render } from '@testing-library/react';

import { queryDefaults } from './api';
import { createAppRouter } from './router';

export function renderRoute(path: string, queries: Record<string, unknown> = { retry: false }) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { ...queryDefaults.queries, refetchInterval: false, ...queries } },
  });
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

export function mockApi(replies: Record<string, Reply | ((body: unknown) => Reply)>) {
  const calls: { method: string; path: string; body: unknown }[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    const path = typeof input === 'string' ? input : input.toString();
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({ method, path, body });
    const entry = replies[`${method} ${path}`];
    const reply = typeof entry === 'function' ? entry(body) : (entry ?? { status: 404, body: { error: 'not_found' } });
    return new Response(reply.status === 204 ? null : JSON.stringify(reply.body ?? null), {
      status: reply.status,
      headers: { 'Content-Type': 'application/json' },
    });
  }) as typeof fetch;
  return calls;
}
