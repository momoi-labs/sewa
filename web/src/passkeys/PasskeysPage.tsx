import {
  Button,
  EmptyState,
  EmptyStateActions,
  EmptyStateDescription,
  EmptyStateTitle,
  PageHeader,
  PageHeaderDescription,
  PageHeaderTitle,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
} from '@momoi-labs/kiso-react';
import { useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import { useState } from 'react';

import { passkeysQuery } from '../api';
import { ErrorAlert } from '../components/ErrorAlert';
import { SkeletonRows } from '../components/SkeletonRows';
import { formatDateTime } from '../format';
import { AddPasskeyDialog } from './AddPasskeyDialog';

const COLUMNS = 3;

/** Every admin's passkeys. Each one can approve changes, on the hosts whose
 *  install pinned it. */
export function PasskeysPage() {
  const passkeys = useQuery(passkeysQuery);
  const [adding, setAdding] = useState(false);
  const add = (
    <Button size="sm" variant="primary" onClick={() => setAdding(true)}>
      <Plus className="icon" aria-hidden="true" />
      Add a passkey
    </Button>
  );

  return (
    <>
      <PageHeader actions={add}>
        <PageHeaderTitle>Passkeys</PageHeaderTitle>
        <PageHeaderDescription>
          A passkey signs each approval. An agent trusts the passkeys that existed when its install script last ran.
        </PageHeaderDescription>
      </PageHeader>
      {passkeys.isError && <ErrorAlert error={passkeys.error} onRetry={() => void passkeys.refetch()} />}
      {passkeys.isSuccess && passkeys.data.length === 0 ? (
        <EmptyState variant="first-run">
          <EmptyStateTitle>No passkeys yet</EmptyStateTitle>
          <EmptyStateDescription>
            Add a passkey, then run the install script on each host again. Agents apply nothing until they trust one.
          </EmptyStateDescription>
          <EmptyStateActions>{add}</EmptyStateActions>
        </EmptyState>
      ) : (
        !(passkeys.isError && !passkeys.data) && (
          <TableFrame>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="col-tight">Name</TableHead>
                  <TableHead>Login</TableHead>
                  <TableHead>Added</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {passkeys.isPending ? (
                  <SkeletonRows columns={COLUMNS} />
                ) : (
                  passkeys.data?.map((passkey) => (
                    <TableRow key={passkey.id}>
                      <TableCell className="col-tight">{passkey.name}</TableCell>
                      <TableCell className="mono">{passkey.login}</TableCell>
                      <TableCell>
                        <time dateTime={passkey.created_at}>{formatDateTime(passkey.created_at)}</time>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
            {passkeys.data && (
              <div className="table-footer">
                {passkeys.data.length === 1 ? '1 passkey' : `${passkeys.data.length} passkeys`}
              </div>
            )}
          </TableFrame>
        )
      )}
      <AddPasskeyDialog open={adding} onOpenChange={setAdding} />
    </>
  );
}
