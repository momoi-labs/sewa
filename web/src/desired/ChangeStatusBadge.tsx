import { StatusBadge, type StatusTone } from '@momoi-labs/kiso-react';

import type { ChangeStatus } from '../api';

const tones: Record<ChangeStatus, StatusTone> = {
  draft: 'neutral',
  approved: 'success',
  discarded: 'neutral',
};

const labels: Record<ChangeStatus, string> = {
  draft: 'Draft',
  approved: 'Approved',
  discarded: 'Discarded',
};

/** A change's status: a draft, approved, or discarded. */
export function ChangeStatusBadge({ status }: { status: ChangeStatus }) {
  return <StatusBadge tone={tones[status]}>{labels[status]}</StatusBadge>;
}
