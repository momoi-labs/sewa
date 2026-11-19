import {
  Button,
  Card,
  CardContent,
  EmptyState,
  EmptyStateDescription,
  EmptyStateTitle,
  Search,
  TableCell,
  TableRow,
} from '@momoi-labs/kiso-react';
import type { ReactNode } from 'react';

export function ListFilters({
  thing,
  query,
  onQuery,
  filtering,
  onClear,
  children,
}: {
  thing: string;
  query: string;
  onQuery: (query: string) => void;
  filtering: boolean;
  onClear: () => void;
  children?: ReactNode;
}) {
  return (
    <div className="list-filters">
      <Search
        aria-label={`Search ${thing}`}
        placeholder={`Search ${thing}…`}
        value={query}
        onChange={(event) => onQuery(event.target.value)}
      />
      {children}
      {filtering && (
        <Button size="sm" variant="ghost" onClick={onClear}>
          Clear filters
        </Button>
      )}
    </div>
  );
}

export function NoMatchRow({ thing, columns }: { thing: string; columns: number }) {
  return (
    <TableRow>
      <TableCell colSpan={columns}>
        <span className="muted">No {thing} match your filters.</span>
      </TableCell>
    </TableRow>
  );
}

export function ListEmpty({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card>
      <CardContent>
        <EmptyState variant="first-run">
          <EmptyStateTitle>{title}</EmptyStateTitle>
          <EmptyStateDescription>{children}</EmptyStateDescription>
        </EmptyState>
      </CardContent>
    </Card>
  );
}

export function countOf(shown: number, all: number, thing: string): string {
  return `${shown} of ${all} ${thing}`;
}
