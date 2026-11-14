import { StatusBadge, type StatusTone } from '@momoi-labs/kiso-react';

import type { AffectedHost, ResultStatus } from '../api';

const tones: Record<ResultStatus, StatusTone> = {
  waiting: 'neutral',
  applied: 'success',
  failed: 'danger',
  replan: 'danger',
};

const labels: Record<ResultStatus, string> = {
  waiting: 'Waiting',
  applied: 'Applied',
  failed: 'Failed',
  replan: 'Needs re-approval',
};

/** What a host did with an approved change. */
export function ApplyResultBadge({ status }: { status: ResultStatus }) {
  return <StatusBadge tone={tones[status]}>{labels[status]}</StatusBadge>;
}

/** "Applied on 3 of 4 hosts, 1 failed", or null before any approval. */
export function applySummary(hosts: AffectedHost[]): string | null {
  const results = hosts.flatMap((host) => (host.result ? [host.result.status] : []));
  if (results.length === 0) return null;
  const count = (status: ResultStatus) => results.filter((result) => result === status).length;
  const total = hosts.length === 1 ? '1 host' : `${hosts.length} hosts`;
  const parts = [`Applied on ${count('applied')} of ${total}`];
  if (count('failed')) parts.push(`${count('failed')} failed`);
  if (count('replan')) parts.push(`${count('replan')} need re-approval`);
  if (count('waiting')) parts.push(`${count('waiting')} waiting`);
  return parts.join(', ');
}
