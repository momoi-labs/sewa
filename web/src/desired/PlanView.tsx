import {
  Alert,
  AlertContent,
  AlertDescription,
  AlertTitle,
  Badge,
  CodeBlock,
  EmptyState,
  EmptyStateDescription,
  EmptyStateTitle,
  Table,
  TableBody,
  TableCell,
  TableFrame,
  TableHead,
  TableHeader,
  TableRow,
} from '@momoi-labs/kiso-react';
import { Minus, Pencil, Plus } from 'lucide-react';

import type { Plan, PlanResource } from '../api';
import { Unknown } from '../components/Unknown';
import { formatDateTime } from '../format';

/** The label of each plan action. A removal is drawn as a danger, since it
 *  takes something off the host. */
function ActionBadge({ action }: { action: string }) {
  switch (action) {
    case 'create':
      return (
        <Badge variant="success">
          <Plus className="icon-sm" aria-hidden="true" />
          Install
        </Badge>
      );
    case 'update':
      return (
        <Badge variant="warning">
          <Pencil className="icon-sm" aria-hidden="true" />
          Update
        </Badge>
      );
    case 'remove':
      return (
        <Badge variant="danger">
          <Minus className="icon-sm" aria-hidden="true" />
          Remove
        </Badge>
      );
    case 'noop':
      return <Badge variant="neutral">No change</Badge>;
    default:
      return <Badge variant="neutral">Unknown</Badge>;
  }
}

const ORDER: Record<string, number> = {
  remove: 0,
  update: 1,
  create: 2,
  unknown: 3,
  noop: 4,
};

/** Removals first, so they are not missed. */
function sorted(resources: PlanResource[]): PlanResource[] {
  return [...resources].sort(
    (a, b) => (ORDER[a.action] ?? 3) - (ORDER[b.action] ?? 3) || a.id.name.localeCompare(b.id.name),
  );
}

/** One sentence for the plan's counts, such as "1 to install, 1 to remove". */
export function summarize(resources: PlanResource[]): string {
  const count = (action: string) => resources.filter((resource) => resource.action === action).length;
  const parts = [
    [count('create'), 'to install'],
    [count('update'), 'to update'],
    [count('remove'), 'to remove'],
    [count('unknown'), 'unknown'],
    [count('noop'), 'unchanged'],
  ] as const;
  const text = parts.filter(([n]) => n > 0).map(([n, label]) => `${n} ${label}`);
  return text.length ? text.join(', ') : 'Nothing to do';
}

/** A mise bootstrap plan as a table: each item, what the host has, what the
 *  config wants, and the action. The agent never applies it. */
export function PlanView({ plan, waiting }: { plan: Plan | null; waiting: string }) {
  if (!plan) {
    return (
      <EmptyState variant="informational">
        <EmptyStateTitle>No plan yet</EmptyStateTitle>
        <EmptyStateDescription>{waiting}</EmptyStateDescription>
      </EmptyState>
    );
  }
  const resources = sorted(plan.plan?.resources ?? []);
  return (
    <div className="stack">
      <p className="row-wrap">
        <span>{summarize(resources)}.</span>
        <span className="muted">Planned {formatDateTime(plan.reported_at)}.</span>
        {plan.stale && <Badge variant="warning">Outdated: a new plan is on its way</Badge>}
      </p>
      {plan.error && (
        <Alert variant="error">
          <AlertContent>
            <AlertTitle>mise could not plan every item</AlertTitle>
            <AlertDescription>
              <CodeBlock code={plan.error} title="mise said" />
            </AlertDescription>
          </AlertContent>
        </Alert>
      )}
      {resources.length > 0 && (
        <TableFrame frame="none">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Item</TableHead>
                <TableHead>On the host now</TableHead>
                <TableHead>The config wants</TableHead>
                <TableHead>Action</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {resources.map((resource) => (
                <TableRow key={`${resource.id.kind}:${resource.id.name}`} data-action={resource.action}>
                  <TableCell className={resource.action === 'remove' ? 'mono danger' : 'mono'}>
                    {resource.id.name}
                  </TableCell>
                  <TableCell className="mono">{resource.current ?? <Unknown reason="mise did not say" />}</TableCell>
                  <TableCell className="mono">{resource.desired ?? <Unknown reason="mise did not say" />}</TableCell>
                  <TableCell>
                    <ActionBadge action={resource.action} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
    </div>
  );
}
