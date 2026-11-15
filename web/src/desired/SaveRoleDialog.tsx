import {
  Badge,
  Button,
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
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
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Minus, Pencil, Plus } from 'lucide-react';

import { type Item, type Role, saveRole } from '../api';
import { ErrorAlert, errorCode } from '../components/ErrorAlert';
import { type Diff, diffSize, itemKey, kindNames } from './items';

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The role as saved. */
  role: Role;
  name: string;
  items: Item[];
  diff: Diff;
  onSaved: (role: Role) => void;
  /** A failure the form shows next to its field, such as a taken name. */
  onFailed: (error: unknown) => void;
};

/** The items a save adds, removes, and changes, and a button to save them. */
export function SaveRoleDialog(props: Props) {
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent>{props.open && <SaveForm {...props} />}</DialogContent>
    </Dialog>
  );
}

function os(item: Item): string {
  return item.os === 'linux' ? 'Linux' : item.os === 'macos' ? 'macOS' : 'Every OS';
}

function SaveForm({ onOpenChange, role, name, items, diff, onSaved, onFailed }: Props) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const mutation = useMutation({
    mutationFn: () => saveRole(role.id, name, items),
    onSuccess: async (saved) => {
      queryClient.setQueryData(['roles', saved.id], saved);
      await Promise.all([['roles'], ['hosts'], ['syncs']].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
      onSaved(saved);
      onOpenChange(false);
      toast('success', `${saved.name} saved`, 'Sync the hosts to apply it.');
    },
    onError: (error) => {
      if (errorCode(error) === 'name_taken') {
        onFailed(error);
        onOpenChange(false);
      }
    },
  });
  const count = diffSize(diff);
  const rows = [
    ...diff.added.map((item) => ({ item, change: 'added' as const, from: null })),
    ...diff.changed.map(({ before, after }) => ({ item: after, change: 'changed' as const, from: before })),
    ...diff.removed.map((item) => ({ item, change: 'removed' as const, from: null })),
  ];

  return (
    <>
      <DialogHeader>
        <DialogTitle>Save {name}?</DialogTitle>
        <DialogDescription>
          {name !== role.name && `The role is renamed from ${role.name}. `}
          {count === 0
            ? 'The items stay the same.'
            : `${count === 1 ? '1 item changes' : `${count} items change`}. No host changes until you sync it.`}
        </DialogDescription>
      </DialogHeader>
      <DialogBody className="stack">
        {mutation.isError && errorCode(mutation.error) !== 'name_taken' && (
          <ErrorAlert error={mutation.error} thing="role" />
        )}
        {rows.length > 0 && (
          <TableFrame frame="none" data-shadow-style="none">
            <Table aria-label="Item changes">
              <TableHeader>
                <TableRow>
                  <TableHead>Change</TableHead>
                  <TableHead>Item</TableHead>
                  <TableHead>Version</TableHead>
                  <TableHead>OS</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map(({ item, change, from }) => (
                  <TableRow key={itemKey(item)}>
                    <TableCell>
                      <ChangeBadge change={change} />
                    </TableCell>
                    <TableCell>
                      <span className="mono">{item.name}</span>
                      <div className="muted">{kindNames[item.kind]}</div>
                    </TableCell>
                    <TableCell className="mono">
                      {from && from.version !== item.version ? `${from.version} to ${item.version}` : item.version}
                    </TableCell>
                    <TableCell>
                      {from && (from.os ?? null) !== (item.os ?? null) ? `${os(from)} to ${os(item)}` : os(item)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableFrame>
        )}
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="ghost" autoFocus disabled={mutation.isPending} onClick={() => onOpenChange(false)}>
          Keep editing
        </Button>
        <Button
          type="button"
          variant="primary"
          aria-busy={mutation.isPending}
          onClick={() => {
            if (!mutation.isPending) mutation.mutate();
          }}
        >
          {mutation.isPending && <Spinner size="sm" />}
          Save
        </Button>
      </DialogFooter>
    </>
  );
}

function ChangeBadge({ change }: { change: 'added' | 'changed' | 'removed' }) {
  switch (change) {
    case 'added':
      return (
        <Badge variant="success">
          <Plus className="icon-sm" aria-hidden="true" />
          Added
        </Badge>
      );
    case 'changed':
      return (
        <Badge variant="warning">
          <Pencil className="icon-sm" aria-hidden="true" />
          Changed
        </Badge>
      );
    default:
      return (
        <Badge variant="danger">
          <Minus className="icon-sm" aria-hidden="true" />
          Removed
        </Badge>
      );
  }
}
