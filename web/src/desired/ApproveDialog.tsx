import {
  Button,
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Skeleton,
  Spinner,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
  useToast,
} from '@momoi-labs/kiso-react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { approvalOptions, approveChange, type ChangeDetail } from '../api';
import { errorCode } from '../components/ErrorAlert';
import { PasskeyError } from '../components/PasskeyError';
import { signApproval } from '../webauthn';

type Props = {
  change: ChangeDetail;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

/** Shows the plans an approval signs, then signs them with a passkey. */
export function ApproveDialog({ change, open, onOpenChange }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>{open && <ApproveForm change={change} onClose={() => onOpenChange(false)} />}</DialogContent>
    </Dialog>
  );
}

function ApproveForm({ change, onClose }: { change: ChangeDetail; onClose: () => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const options = useQuery({
    queryKey: ['changes', change.id, 'approval'],
    queryFn: () => approvalOptions(change.id),
    gcTime: 0,
    retry: false,
  });
  const mutation = useMutation({
    mutationFn: async () => approveChange(change.id, await signApproval(options.data!)),
    onSuccess: async () => {
      await Promise.all(
        [['changes'], ['roles'], ['hosts']].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
      onClose();
      toast('success', 'Change approved', 'Each agent checks the signature, then applies its plan.');
    },
    onError: (error) => {
      if (errorCode(error) === 'plans_changed') void options.refetch();
    },
  });
  const names = new Map(change.hosts.map((host) => [host.id, host.name]));
  const hosts = options.data?.hosts ?? [];
  const count = hosts.length === 1 ? '1 host' : `${hosts.length} hosts`;

  return (
    <>
      <DialogHeader>
        <DialogTitle>Approve the change?</DialogTitle>
        <DialogDescription>
          {options.data
            ? `Your passkey signs these plans. ${count === '1 host' ? 'The host applies' : `The ${count} apply`} them with mise bootstrap. A host whose plan changes first does not apply.`
            : 'Your passkey signs the plan of each affected host.'}
        </DialogDescription>
      </DialogHeader>
      <DialogBody className="stack">
        {options.isError && <PasskeyError error={options.error} thing="change" />}
        {mutation.isError && <PasskeyError error={mutation.error} thing="change" />}
        {options.isPending ? (
          <Skeleton />
        ) : (
          options.data && (
            <TableFrame frame="none">
              <Table aria-label="Plans to sign">
                <TableHeader>
                  <TableRow>
                    <TableHead>Host</TableHead>
                    <TableHead>Plan</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {hosts.map((host) => (
                    <TableRow key={host.host_id}>
                      <TableCell>{names.get(host.host_id) ?? host.host_id}</TableCell>
                      <TableCell className="mono" title={host.plan_hash}>
                        {host.plan_hash.slice(0, 12)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TableFrame>
          )
        )}
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="ghost" autoFocus disabled={mutation.isPending} onClick={onClose}>
          Cancel
        </Button>
        <Button
          type="button"
          variant="primary"
          disabled={!options.data}
          aria-busy={mutation.isPending}
          onClick={() => {
            if (!mutation.isPending && options.data) mutation.mutate();
          }}
        >
          {mutation.isPending && <Spinner size="sm" />}
          Approve with passkey
        </Button>
      </DialogFooter>
    </>
  );
}
