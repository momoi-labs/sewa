import { Alert, AlertContent, AlertDescription, AlertTitle, Button } from '@momoi-labs/kiso-react';

import { ApiFailure } from '../api';

/** What happened, why, and what to do now, by the API's error code. */
const messages: Record<string, { title: string; body: string }> = {
  network: {
    title: 'The controller did not answer',
    body: 'The browser could not reach sewa-controller. Check that it runs, then try again.',
  },
  not_admin: {
    title: 'You are not an admin',
    body: 'Your Tailscale login is not in SEWA_ADMINS. Ask the person who runs the controller to add it.',
  },
  not_tailnet: {
    title: 'You are not on the tailnet',
    body: 'Tailscale does not know this connection. Connect this device to the tailnet, then try again.',
  },
  whois_unavailable: {
    title: 'The controller cannot check who you are',
    body: 'tailscaled on the controller did not answer. Check that Tailscale runs there, then try again.',
  },
  not_found: {
    title: 'This host does not exist',
    body: 'No host has this address. Go back to the host list and pick one.',
  },
  internal: {
    title: 'Something went wrong on the controller',
    body: 'The controller log has the details. Try again in a moment.',
  },
};

export function errorCode(error: unknown): string {
  return error instanceof ApiFailure ? error.code : 'internal';
}

/** An error Alert for a failed API call, with an optional retry. */
export function ErrorAlert({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const message = messages[errorCode(error)] ?? messages.internal!;
  return (
    <Alert variant="error">
      <AlertContent>
        <AlertTitle>{message.title}</AlertTitle>
        <AlertDescription>{message.body}</AlertDescription>
      </AlertContent>
      {onRetry && (
        <Button size="sm" onClick={onRetry}>
          Try again
        </Button>
      )}
    </Alert>
  );
}
