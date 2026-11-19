import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Button,
  Dialog,
  DialogContent,
  Lifecycle,
  Spinner,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { type ReactNode, useState } from 'react';

import { type Created, deleteHost, disableHost, enableHost, type Host } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { InstallStep } from './AddHostDialog';
import { HostStatusBadge } from './HostStatusBadge';

export function HostActions({ host, children }: { host: Host; children: ReactNode }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const [deleting, setDeleting] = useState(false);
  const [created, setCreated] = useState<Created | null>(null);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['hosts'] });

  const disable = useMutation({
    mutationFn: () => disableHost(host.id),
    onSuccess: async () => {
      await refresh();
      toast('success', `${host.name} disabled`, 'Its credentials are revoked. Its history stays.');
    },
  });
  const enable = useMutation({
    mutationFn: () => enableHost(host.id),
    onSuccess: async (result) => {
      setCreated(result);
      await refresh();
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteHost(host.id),
    onSuccess: async () => {
      setDeleting(false);
      await navigate({ to: '/' });
      queryClient.removeQueries({ queryKey: ['hosts', host.id] });
      await refresh();
      toast('success', `${host.name} deleted`);
    },
  });
  const failure = disable.error ?? enable.error;

  return (
    <>
      <div className="between">
        {children}
        <Lifecycle
          status={<HostStatusBadge status={host.status} />}
          actionsLabel="Host actions"
          actions={
            <>
              {host.status === 'disabled' ? (
                <Button size="sm" aria-busy={enable.isPending} onClick={() => enable.mutate()}>
                  {enable.isPending && <Spinner size="sm" />}
                  Enable
                </Button>
              ) : (
                <Button size="sm" aria-busy={disable.isPending} onClick={() => disable.mutate()}>
                  {disable.isPending && <Spinner size="sm" />}
                  Disable
                </Button>
              )}
              <Button
                size="sm"
                variant="ghost"
                className="btn-danger-ghost"
                onClick={() => {
                  remove.reset();
                  setDeleting(true);
                }}
              >
                Delete host
              </Button>
            </>
          }
        />
      </div>
      {failure && <ErrorAlert error={failure} />}
      <Dialog open={created !== null} onOpenChange={(open) => !open && setCreated(null)}>
        <DialogContent>{created && <InstallStep created={created} onClose={() => setCreated(null)} />}</DialogContent>
      </Dialog>
      <AlertDialog open={deleting} onOpenChange={(open) => !remove.isPending && setDeleting(open)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {host.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              This removes the host with its events, inventory, roles, plans and tokens. A sync that covers only this
              host goes too. This cannot be undone. Disable the host instead to keep its history.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {remove.isError && <ErrorAlert error={remove.error} />}
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              aria-busy={remove.isPending}
              onClick={(event) => {
                event.preventDefault();
                if (!remove.isPending) remove.mutate();
              }}
            >
              {remove.isPending && <Spinner size="sm" />}
              Delete host
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
