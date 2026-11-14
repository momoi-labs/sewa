import {
  Button,
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Form,
  FormField,
  Spinner,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { type FormEvent, useState } from 'react';

import { ApiFailure, passkeyOptions, passkeysQuery, registerPasskey } from '../api';
import { PasskeyError } from '../components/PasskeyError';
import { createPasskey } from '../webauthn';

type Props = { open: boolean; onOpenChange: (open: boolean) => void };

/** Names a new passkey, then lets the browser create it. */
export function AddPasskeyDialog({ open, onOpenChange }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>{open && <AddPasskeyForm onClose={() => onOpenChange(false)} />}</DialogContent>
    </Dialog>
  );
}

function AddPasskeyForm({ onClose }: { onClose: () => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [name, setName] = useState('');
  const [checked, setChecked] = useState(false);
  const mutation = useMutation({
    mutationFn: async () => {
      const options = await passkeyOptions();
      return registerPasskey(await createPasskey(options, name.trim()));
    },
    onSuccess: async (passkey) => {
      await queryClient.invalidateQueries({ queryKey: passkeysQuery.queryKey });
      onClose();
      toast('success', 'Passkey added', `Run the install script on each host again so its agent trusts ${passkey.name}.`);
    },
  });
  const code = mutation.error instanceof ApiFailure ? mutation.error.code : null;
  const nameError =
    checked && !name.trim()
      ? 'Enter a name.'
      : code === 'invalid_name'
        ? 'Use up to 64 characters, without control characters.'
        : undefined;

  function submit(event: FormEvent) {
    event.preventDefault();
    setChecked(true);
    if (mutation.isPending || !name.trim()) return;
    mutation.mutate();
  }

  return (
    <Form onSubmit={submit} noValidate>
      <DialogHeader>
        <DialogTitle>Add a passkey</DialogTitle>
        <DialogDescription>
          Your browser asks an authenticator, such as this device or a security key, to create the passkey.
        </DialogDescription>
      </DialogHeader>
      <DialogBody className="stack">
        {mutation.isError && code !== 'invalid_name' && <PasskeyError error={mutation.error} />}
        <FormField
          name="name"
          label="Name"
          placeholder="Laptop"
          hint="Say which device holds it."
          autoComplete="off"
          autoFocus
          maxLength={64}
          value={name}
          onChange={(event) => setName(event.target.value)}
          error={nameError}
        />
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="ghost" disabled={mutation.isPending} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" aria-busy={mutation.isPending}>
          {mutation.isPending && <Spinner size="sm" />}
          Create passkey
        </Button>
      </DialogFooter>
    </Form>
  );
}
