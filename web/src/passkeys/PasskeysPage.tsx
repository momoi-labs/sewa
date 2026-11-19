import {
  Button,
  PageHeader,
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
import { countOf, ListEmpty, ListFilters, NoMatchRow } from '../components/ListFilters';
import { SkeletonRows } from '../components/SkeletonRows';
import { formatDateTime } from '../format';
import { AddPasskeyDialog } from './AddPasskeyDialog';

const COLUMNS = 3;

export function PasskeysPage() {
  const passkeys = useQuery(passkeysQuery);
  const [adding, setAdding] = useState(false);
  const [query, setQuery] = useState('');
  const typed = query.trim().toLowerCase();
  const shown = (passkeys.data ?? []).filter((passkey) => `${passkey.name} ${passkey.login}`.toLowerCase().includes(typed));
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
      </PageHeader>
      {passkeys.isError && <ErrorAlert error={passkeys.error} onRetry={() => void passkeys.refetch()} />}
      {passkeys.isSuccess && passkeys.data.length === 0 ? (
        <ListEmpty title="No passkeys yet">Agents apply nothing until they trust a passkey.</ListEmpty>
      ) : (
        !(passkeys.isError && !passkeys.data) && (
          <>
            <ListFilters
              thing="passkeys"
              query={query}
              onQuery={setQuery}
              filtering={typed !== ''}
              onClear={() => setQuery('')}
            />
          <TableFrame data-fill="false">
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
                ) : shown.length === 0 ? (
                  <NoMatchRow thing="passkeys" columns={COLUMNS} />
                ) : (
                  shown.map((passkey) => (
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
              <div className="table-footer">{countOf(shown.length, passkeys.data.length, 'passkeys')}</div>
            )}
          </TableFrame>
          </>
        )
      )}
      <AddPasskeyDialog open={adding} onOpenChange={setAdding} />
    </>
  );
}
