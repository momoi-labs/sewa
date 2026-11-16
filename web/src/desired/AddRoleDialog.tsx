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
} from '@momoi-labs/kiso-react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { type FormEvent, useState } from 'react';

import { ApiFailure, createRole, rolesQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';

const NAME = /^[A-Za-z0-9._-]{1,63}$/;

type Props = { open: boolean; onOpenChange: (open: boolean) => void };

export function AddRoleDialog({ open, onOpenChange }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>{open && <AddRoleForm onClose={() => onOpenChange(false)} />}</DialogContent>
    </Dialog>
  );
}

function AddRoleForm({ onClose }: { onClose: () => void }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState('');
  const [checked, setChecked] = useState(false);
  const mutation = useMutation({
    mutationFn: () => createRole(name.trim()),
    onSuccess: async (role) => {
      await queryClient.invalidateQueries({ queryKey: rolesQuery.queryKey });
      onClose();
      await navigate({ to: '/roles/$roleId', params: { roleId: role.id } });
    },
  });
  const code = mutation.error instanceof ApiFailure ? mutation.error.code : null;
  const nameError =
    checked && !name.trim()
      ? 'Enter a name.'
      : checked && !NAME.test(name.trim())
        ? 'Use up to 63 letters, digits, dots, dashes or underscores.'
        : code === 'name_taken'
          ? 'A role has this name. Choose another.'
          : undefined;

  function submit(event: FormEvent) {
    event.preventDefault();
    setChecked(true);
    if (mutation.isPending || !NAME.test(name.trim())) return;
    mutation.mutate();
  }

  return (
    <Form onSubmit={submit} noValidate>
      <DialogHeader>
        <DialogTitle>Add a role</DialogTitle>
        <DialogDescription>A role is a set of packages and mise tools that you give to hosts.</DialogDescription>
      </DialogHeader>
      <DialogBody className="stack">
        {mutation.isError && code !== 'name_taken' && <ErrorAlert error={mutation.error} />}
        <FormField
          name="name"
          label="Name"
          placeholder="base"
          hint="Letters, digits, dots, dashes and underscores."
          autoComplete="off"
          autoFocus
          maxLength={63}
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
          Add role
        </Button>
      </DialogFooter>
    </Form>
  );
}
