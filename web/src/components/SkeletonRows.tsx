import { Skeleton, TableCell, TableRow } from '@momoi-labs/kiso-react';

/** Skeleton rows aligned to the table's columns, while the rows load. */
export function SkeletonRows({ columns, rows = 3 }: { columns: number; rows?: number }) {
  return Array.from({ length: rows }, (_, row) => (
    <TableRow key={row}>
      {Array.from({ length: columns }, (_, column) => (
        <TableCell key={column}>
          <Skeleton />
        </TableCell>
      ))}
    </TableRow>
  ));
}
