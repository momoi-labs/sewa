import { Button, EmptyState, EmptyStateActions, EmptyStateDescription, EmptyStateTitle } from '@momoi-labs/kiso-react';
import { Link } from '@tanstack/react-router';

export function NotFound() {
  return (
    <EmptyState variant="informational">
      <EmptyStateTitle>This page does not exist</EmptyStateTitle>
      <EmptyStateDescription>The address names no page of sewa.</EmptyStateDescription>
      <EmptyStateActions>
        <Button asChild>
          <Link to="/">Go to the hosts</Link>
        </Button>
      </EmptyStateActions>
    </EmptyState>
  );
}
