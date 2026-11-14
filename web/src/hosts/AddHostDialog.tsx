import {
  Alert,
  AlertContent,
  AlertDescription,
  AlertTitle,
  Button,
  CodeBlock,
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Form,
  FormField,
  RadioGroup,
  RadioGroupItem,
  Spinner,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { type FormEvent, useState } from 'react';

import { ApiFailure, type Created, createHost, hostsQuery, type Os } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { formatClock } from '../format';
import { HostStatusBadge } from './HostStatusBadge';

/** A name the controller takes: 1 to 63 letters, digits, `-`, `_` or `.`. */
const NAME = /^[A-Za-z0-9._-]{1,63}$/;

type Props = { open: boolean; onOpenChange: (open: boolean) => void };

/** Creates a host, then shows its install command once. */
export function AddHostDialog({ open, onOpenChange }: Props) {
  // A pending creation would show its command in a dialog the person
  // already closed, so the dialog stays until the answer arrives.
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open={open} onOpenChange={(next) => (next || !busy) && onOpenChange(next)}>
      <DialogContent>
        {open && <AddHostForm onBusy={setBusy} onClose={() => onOpenChange(false)} />}
      </DialogContent>
    </Dialog>
  );
}

function AddHostForm({ onBusy, onClose }: { onBusy: (busy: boolean) => void; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [os, setOs] = useState<Os>('linux');
  const [checked, setChecked] = useState(false);
  const [created, setCreated] = useState<Created | null>(null);
  const mutation = useMutation({
    mutationFn: () => createHost(name.trim(), os),
    onMutate: () => onBusy(true),
    onSettled: () => onBusy(false),
    onSuccess: async (result) => {
      setCreated(result);
      await queryClient.invalidateQueries({ queryKey: hostsQuery.queryKey });
    },
  });
  const code = mutation.error instanceof ApiFailure ? mutation.error.code : null;
  const nameError =
    checked && !name.trim()
      ? 'Enter a name.'
      : checked && !NAME.test(name.trim())
        ? 'Use up to 63 letters, digits, dots, dashes or underscores.'
        : code === 'name_taken'
          ? 'A host has this name. Choose another.'
          : undefined;

  function submit(event: FormEvent) {
    event.preventDefault();
    setChecked(true);
    if (mutation.isPending || !NAME.test(name.trim())) return;
    mutation.mutate();
  }

  if (created) return <InstallStep created={created} onClose={onClose} />;

  return (
    <Form onSubmit={submit} noValidate>
      <DialogHeader>
        <DialogTitle>Add a host</DialogTitle>
        <DialogDescription>sewa gives you a command to run on the host. It installs the agent.</DialogDescription>
      </DialogHeader>
      <DialogBody className="stack">
        {mutation.isError && code !== 'name_taken' && <ErrorAlert error={mutation.error} />}
        <FormField
          name="name"
          label="Name"
          placeholder="build-1"
          hint="Letters, digits, dots, dashes and underscores."
          autoComplete="off"
          autoFocus
          maxLength={63}
          value={name}
          onChange={(event) => setName(event.target.value)}
          error={nameError}
        />
        <RadioGroup label="Operating system" variant="segmented" value={os} onValueChange={(value) => setOs(value as Os)}>
          <RadioGroupItem value="linux" label="Linux" />
          <RadioGroupItem value="macos" label="macOS" />
        </RadioGroup>
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="ghost" disabled={mutation.isPending} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" aria-busy={mutation.isPending}>
          {mutation.isPending && <Spinner size="sm" />}
          Add host
        </Button>
      </DialogFooter>
    </Form>
  );
}

/** The install command of a new host, with its live status until the agent answers. */
function InstallStep({ created, onClose }: { created: Created; onClose: () => void }) {
  const hosts = useQuery(hostsQuery);
  const host = hosts.data?.find((item) => item.id === created.host.id) ?? created.host;
  return (
    <>
      <DialogHeader>
        <DialogTitle>Install the agent on {host.name}</DialogTitle>
        <DialogDescription>
          Run this command on {host.name} in a terminal. It needs sudo.
        </DialogDescription>
      </DialogHeader>
      <DialogBody className="stack">
        <Alert variant="warning">
          <AlertContent>
            <AlertTitle>The command shows only once</AlertTitle>
            <AlertDescription>
              Its token works once and is valid for 1 hour, until {formatClock(created.token_expires_at)}. Copy it
              now.
            </AlertDescription>
          </AlertContent>
        </Alert>
        <CodeBlock terminal code={created.install_command} copy="command" />
        <p className="row-wrap" role="status">
          <HostStatusBadge status={host.status} />
          <span className="muted">
            {host.status === 'waiting'
              ? 'This updates when the agent enrolls.'
              : `The agent on ${host.name} answered.`}
          </span>
        </p>
      </DialogBody>
      <DialogFooter>
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </DialogFooter>
    </>
  );
}
