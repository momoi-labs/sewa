import { Button, EmptyStateActions, EmptyStateDescription, EmptyStateTitle } from '@momoi-labs/kiso-react';
import { useId } from 'react';

import { ApiFailure } from '../api';
import './ErrorPage.css';
import angry from './error-art/error-403.svg?inline';
import angryDark from './error-art/error-403-dark.svg?inline';
import repair from './error-art/error-500.svg?inline';
import repairDark from './error-art/error-500-dark.svg?inline';
import surprised from './error-art/error-offline.svg?inline';
import surprisedDark from './error-art/error-offline-dark.svg?inline';

/** The art is inlined in the bundle: the page often shows when the
 *  controller that serves the files is down. */
const arts = {
  angry: [angry, angryDark],
  repair: [repair, repairDark],
  surprised: [surprised, surprisedDark],
} as const;

/** The cat for an error: angry for a 403, repair for a 5xx, and surprised
 *  for anything else, such as no answer or a 404. */
export function errorArt(error: unknown): keyof typeof arts {
  if (!(error instanceof ApiFailure)) return 'surprised';
  if (error.status === 403 || error.code === 'not_admin') return 'angry';
  if (error.status >= 500) return 'repair';
  return 'surprised';
}

/** The full page for a load that failed with `not_admin`, a 5xx, or no answer:
 *  a brand illustration, the status code, what happened, and one action. It
 *  has no shell around it. */
export function ErrorPage({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const titleId = useId();
  const status = error instanceof ApiFailure ? error.status : 500;
  const denied = error instanceof ApiFailure && error.code === 'not_admin';
  const art = errorArt(error);
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
    <main className="error-page-screen hatch">
      <div
        className="empty error-page"
        role={denied ? 'region' : 'alert'}
        aria-labelledby={titleId}
      >
        <img className="error-page-art" data-art={art} src={arts[art][0]} alt="" />
        <img className="error-page-art error-page-art-dark" src={arts[art][1]} alt="" />
        <p className="mono">{status === 0 ? 'No answer' : `Error ${status}`}</p>
        <EmptyStateTitle id={titleId}>{page.title}</EmptyStateTitle>
        <EmptyStateDescription>{page.body}</EmptyStateDescription>
        <EmptyStateActions>
          <Button onClick={onRetry}>{page.action}</Button>
        </EmptyStateActions>
      </div>
    </main>
  );
}
