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

const HOST = 'host';

type Props = {
  host: Host;
  pkg: string | null;
  onClose: () => void;
};

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
        [['syncs'], ['roles'], ['hosts']].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
      onClose();
      toast('success', `${pkg} adopted`, `It is part of the desired state now. Sync to apply it.`);
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
          {pkg} joins the desired state at version latest. A role gives it to every host with the role.
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
              {(roles.data ?? []).filter((role) => !role.builtin).map((role) => (
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
          Adopt
        </Button>
      </DialogFooter>
    </Form>
  );
}
