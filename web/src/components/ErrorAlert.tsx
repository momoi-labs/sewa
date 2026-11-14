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
  invalid_item: {
    title: 'The item was not saved',
    body: 'Its name or version is empty, too long, or has spaces at either end, or a tool was set to absent. Fix it, then try again.',
  },
  unknown_role: {
    title: 'The role no longer exists',
    body: 'Reload the page and pick another role.',
  },
  unknown_host: {
    title: 'The host no longer exists',
    body: 'Reload the page and pick another host.',
  },
  not_draft: {
    title: 'The change is closed',
    body: 'Someone discarded this change already. Reload the page to see it.',
  },
  invalid_name: {
    title: 'The name was not saved',
    body: 'Give the passkey a name of 1 to 64 characters, then try again.',
  },
  unsupported_algorithm: {
    title: 'This passkey type is not supported',
    body: 'Agents check ES256 passkeys only. Use another authenticator, such as a phone or a security key.',
  },
  invalid_passkey: {
    title: 'The passkey was not saved',
    body: 'The controller could not read what the browser sent. Try again, or use another authenticator.',
  },
  unknown_challenge: {
    title: 'The passkey request expired',
    body: 'A passkey must be created within five minutes. Try again.',
  },
  wrong_origin: {
    title: 'The address does not match the controller',
    body: 'Open the web app at the address in SEWA_PUBLIC_URL, then try again.',
  },
  passkey_exists: {
    title: 'This passkey is saved already',
    body: 'Use another authenticator to add a second passkey.',
  },
  no_passkey: {
    title: 'You have no passkey',
    body: 'Add a passkey on the Passkeys page, then run the install script on each host again so its agent trusts it.',
  },
  not_approvable: {
    title: 'This change cannot be approved',
    body: 'It was discarded, or every host has its approval already. Reload the page to see it.',
  },
  nothing_to_approve: {
    title: 'No host needs this change',
    body: 'The change does not alter the config of any host. Edit a role or a host first.',
  },
  plans_not_ready: {
    title: 'Some hosts have not planned the change',
    body: 'Wait until each affected host shows its plan, then approve.',
  },
  plans_changed: {
    title: 'A plan changed while you approved',
    body: 'Check the new plans, then approve again.',
  },
  invalid_assertion: {
    title: 'The controller refused the signature',
    body: 'Use one of your own passkeys, then try again.',
  },
  internal: {
    title: 'Something went wrong on the controller',
    body: 'The controller log has the details. Try again in a moment.',
  },
};

export function errorCode(error: unknown): string {
  return error instanceof ApiFailure ? error.code : 'internal';
}

/** An error Alert for a failed API call, with an optional retry. `thing`
 *  names what a 404 did not find: a host, a role, or a change. */
export function ErrorAlert({
  error,
  onRetry,
  thing = 'host',
}: {
  error: unknown;
  onRetry?: () => void;
  thing?: string;
}) {
  const code = errorCode(error);
  const message =
    code === 'not_found'
      ? {
          title: `This ${thing} does not exist`,
          body: `No ${thing} has this address. Go back to the ${thing} list and pick one.`,
        }
      : (messages[code] ?? messages.internal!);
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
