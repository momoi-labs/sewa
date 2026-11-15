import { Button, EmptyState, EmptyStateActions, EmptyStateDescription, EmptyStateTitle } from '@momoi-labs/kiso-react';
import { useId } from 'react';

import { ApiFailure } from '../api';
import './ErrorPage.css';

/** True when a failed load should take the whole page: the person is not an
 *  admin, the controller failed, or it did not answer. */
export function isPageError(error: unknown): boolean {
  if (!(error instanceof ApiFailure)) return true;
  return error.code === 'not_admin' || error.code === 'network' || error.status >= 500;
}

/** The page for a load that failed with `not_admin`, a 5xx, or no answer:
 *  a brand illustration, the status code, what happened, and one action. */
export function ErrorPage({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const titleId = useId();
  const status = error instanceof ApiFailure ? error.status : 500;
  const denied = error instanceof ApiFailure && error.code === 'not_admin';
  const art = denied ? 'error-403' : 'error-500';
  const page = denied
    ? {
        title: 'The guardian said no',
        body: 'This area is restricted to sewa admins. Ask the person who runs the controller to add your login to SEWA_ADMINS.',
        action: 'Check again',
      }
    : status === 0
      ? {
          title: 'The controller did not answer',
          body: 'The guardian is looking into it. Check that sewa-controller runs, then retry.',
          action: 'Retry',
        }
      : {
          title: 'Something broke on our side',
          body: 'The guardian is fixing it. The controller log has the details. Retry in a moment.',
          action: 'Retry',
        };

  return (
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
  );
}
