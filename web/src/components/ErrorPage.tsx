import { Button, EmptyState, EmptyStateActions, EmptyStateDescription, EmptyStateTitle } from '@momoi-labs/kiso-react';
import { useId } from 'react';

import { ApiFailure } from '../api';
import './ErrorPage.css';

/** The full page for a load that failed with `not_admin`, a 5xx, or no answer:
 *  a brand illustration, the status code, what happened, and one action. It
 *  has no shell around it. */
export function ErrorPage({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const titleId = useId();
  const status = error instanceof ApiFailure ? error.status : 500;
  const denied = error instanceof ApiFailure && error.code === 'not_admin';
  const art = denied ? 'error-403' : status === 0 ? 'error-offline' : 'error-500';
  const page = denied
    ? {
        title: 'Access denied',
        body: 'Ask an admin to add your login to SEWA_ADMINS.',
        action: 'Check again',
      }
    : status === 0
      ? {
          title: 'The controller did not answer',
          body: 'Check that the controller is running, then retry.',
          action: 'Retry',
        }
      : {
          title: 'Something broke on our side',
          body: 'Check the controller log, then retry.',
          action: 'Retry',
        };

  return (
    <main className="error-page-screen">
      <EmptyState
        variant="informational"
        className="error-page"
        role={denied ? 'region' : 'alert'}
        aria-labelledby={titleId}
      >
        <img className="error-page-art" src={`/${art}.svg`} alt="" />
        <img className="error-page-art error-page-art-dark" src={`/${art}-dark.svg`} alt="" />
        <p className="mono">{status === 0 ? 'No answer' : `Error ${status}`}</p>
        <EmptyStateTitle id={titleId}>{page.title}</EmptyStateTitle>
        <EmptyStateDescription>{page.body}</EmptyStateDescription>
        <EmptyStateActions>
          <Button onClick={onRetry}>{page.action}</Button>
        </EmptyStateActions>
      </EmptyState>
    </main>
  );
}
