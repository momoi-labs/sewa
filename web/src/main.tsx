import '@momoi-labs/kiso-react/styles.css';

import { Toasts } from '@momoi-labs/kiso-react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { createAppRouter } from './router';

const queryClient = new QueryClient();
const router = createAppRouter(queryClient);

const root = document.getElementById('root');
if (!root) throw new Error('The page has no #root element.');

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <Toasts>
        <RouterProvider router={router} />
      </Toasts>
    </QueryClientProvider>
  </StrictMode>,
);
