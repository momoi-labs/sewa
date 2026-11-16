import { Alert, AlertContent, AlertDescription, AlertTitle } from '@momoi-labs/kiso-react';

import { PasskeyFailure } from '../webauthn';
import { ErrorAlert } from './ErrorAlert';

const messages: Record<string, { title: string; body: string }> = {
  unsupported: {
    title: 'This browser cannot use passkeys here',
    body: 'Passkeys need HTTPS or localhost and a browser that supports them. Open the web app at its HTTPS address.',
  },
  cancelled: {
    title: 'The passkey prompt closed',
    body: 'The browser got no answer from an authenticator. Try again and confirm on the device.',
  },
  exists: {
    title: 'This authenticator has a passkey already',
    body: 'Use another authenticator to add a second passkey.',
  },
  failed: {
    title: 'The passkey did not work',
    body: 'The browser reported an error. Try again, or use another authenticator.',
  },
};

export function PasskeyError({ error, thing }: { error: unknown; thing?: string }) {
  if (!(error instanceof PasskeyFailure)) return <ErrorAlert error={error} thing={thing} />;
  const message = messages[error.reason] ?? messages.failed!;
  return (
    <Alert variant={error.reason === 'cancelled' ? 'warning' : 'error'}>
      <AlertContent>
        <AlertTitle>{message.title}</AlertTitle>
        <AlertDescription>{message.body}</AlertDescription>
      </AlertContent>
    </Alert>
  );
}
