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
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Spinner,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { type FormEvent, useState } from 'react';

import { adoptPackage, type Host, rolesQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';

/** The Select value for this host's adjustments. Role ids are UUIDs. */
const HOST = 'host';

type Props = {
  host: Host;
  /** The undeclared package to adopt, or null when the dialog is closed. */
  pkg: string | null;
  onClose: () => void;
};

/** Adds an undeclared package to a role or to the host's adjustments, as an
 *  edit of the draft. */
export function AdoptDialog({ host, pkg, onClose }: Props) {
  return (
    <Dialog open={pkg !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent>{pkg !== null && <AdoptForm host={host} pkg={pkg} onClose={onClose} />}</DialogContent>
    </Dialog>
  );
}

function AdoptForm({ host, pkg, onClose }: { host: Host; pkg: string; onClose: () => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const roles = useQuery(rolesQuery);
  const [target, setTarget] = useState(HOST);
  const mutation = useMutation({
    mutationFn: () => adoptPackage(host.id, pkg, target === HOST ? null : target),
    onSuccess: async () => {
      await Promise.all(
        [['changes'], ['roles'], ['hosts']].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
      onClose();
      toast('success', 'Package added to the draft', `Approve the draft to make ${pkg} part of the desired state.`);
    },
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!mutation.isPending) mutation.mutate();
  }

  return (
    <Form onSubmit={submit} noValidate>
      <DialogHeader>
        <DialogTitle>Adopt {pkg}?</DialogTitle>
        <DialogDescription>
          The draft gets {pkg} at version latest. A role gives it to every host with the role.
        </DialogDescription>
      </DialogHeader>
      <DialogBody className="stack">
        {mutation.isError && <ErrorAlert error={mutation.error} />}
        {roles.isError && <ErrorAlert error={roles.error} onRetry={() => void roles.refetch()} />}
        <div className="stack-xs">
          <Label htmlFor="adopt-target">Add it to</Label>
          <Select value={target} onValueChange={setTarget}>
            <SelectTrigger id="adopt-target">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={HOST}>Only {host.name}</SelectItem>
              {(roles.data ?? []).map((role) => (
                <SelectItem key={role.id} value={role.id}>
                  Role {role.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="ghost" autoFocus disabled={mutation.isPending} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" aria-busy={mutation.isPending}>
          {mutation.isPending && <Spinner size="sm" />}
          Add to draft
        </Button>
      </DialogFooter>
    </Form>
  );
}
