import {
  Button,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
} from '@momoi-labs/kiso-react';

import type { ItemKind } from '../api';
import type { ItemRow } from './compare';
import { DifferenceBadge } from './DifferenceBadge';

const kinds: Record<ItemKind, string> = {
  package: 'Package',
  tool: 'mise tool',
};

/** Items with the approved version beside the draft's, and a remove action. */
export function ItemsTable({
  rows,
  label,
  onRemove,
  removeLabel = 'Remove',
}: {
  rows: ItemRow[];
  label: string;
  onRemove?: (row: ItemRow) => void;
  removeLabel?: string;
}) {
  return (
    <TableFrame frame="none">
      <Table aria-label={label}>
        <TableHeader>
          <TableRow>
            <TableHead>Item</TableHead>
            <TableHead>Kind</TableHead>
            <TableHead>Approved</TableHead>
            <TableHead>In draft</TableHead>
            <TableHead>Draft</TableHead>
            {onRemove && <TableHead aria-label="Actions" />}
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={`${row.kind}:${row.name}`}>
              <TableCell className="mono">{row.name}</TableCell>
              <TableCell>{kinds[row.kind]}</TableCell>
              <TableCell className="mono">{row.before ?? <span className="muted">Not set</span>}</TableCell>
              <TableCell className="mono">{row.after ?? <span className="muted">Not set</span>}</TableCell>
              <TableCell>
                <DifferenceBadge difference={row.difference} />
              </TableCell>
              {onRemove && (
                <TableCell>
                  {row.after !== null && (
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => onRemove(row)}
                      aria-label={`${removeLabel} ${row.name}`}
                    >
                      {removeLabel}
                    </Button>
                  )}
                </TableCell>
              )}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </TableFrame>
  );
}
