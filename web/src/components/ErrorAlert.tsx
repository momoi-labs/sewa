import { Alert, AlertContent, AlertDescription, AlertTitle, Button } from '@momoi-labs/kiso-react';

import { ApiFailure } from '../api';

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
    body: 'Its name or version is empty, too long, or has spaces, or a mise tool was set to absent. Fix it, then try again.',
  },
  unknown_role: {
    title: 'The role no longer exists',
    body: 'Reload the page and pick another role.',
  },
  unknown_host: {
    title: 'The host no longer exists',
    body: 'Reload the page and pick another host.',
  },
  invalid_name: {
    title: 'The name was not saved',
    body: 'The name is empty, too long, or has characters the controller does not take. Fix it, then try again.',
  },
  name_taken: {
    title: 'The name is taken',
    body: 'Another role has this name. Choose another, then save again.',
  },
  duplicate_item: {
    title: 'An item is listed twice',
    body: 'Each package and each mise tool can be in a role once. Remove the copy, then save again.',
  },
  too_many_items: {
    title: 'The role has too many items',
    body: 'Split the items into two roles, then save again.',
  },
  builtin_role: {
    title: 'The built-in role is read-only',
    body: 'sewa manages the built-in role. Put the item in another role.',
  },
  invalid_edit: {
    title: 'The edit was not saved',
    body: 'The controller could not read it. Reload the page, then try again.',
  },
  too_many_edits: {
    title: 'Too many edits at once',
    body: 'Save fewer edits at a time.',
  },
  no_hosts: {
    title: 'No host is picked',
    body: 'Pick at least one host, then start the sync.',
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
  nothing_to_approve: {
    title: 'Nothing to sign',
    body: 'No host in this sync has a plan with changes. Reload the page to see where each host is.',
  },
  plans_changed: {
    title: 'A plan changed while you signed',
    body: 'Check the new plans, then sign again.',
  },
  stale_plans: {
    title: 'The plans changed',
    body: 'The controller has newer plans than this page shows, so nothing was signed. Reload the page, check the plans, then sign again.',
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
