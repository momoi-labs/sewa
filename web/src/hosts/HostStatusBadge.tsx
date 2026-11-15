import { StatusBadge, type StatusTone } from '@momoi-labs/kiso-react';

import type { HostStatus } from '../api';

const tones: Record<HostStatus, StatusTone> = {
  waiting: 'neutral',
  connected: 'success',
  disconnected: 'danger',
};

const labels: Record<HostStatus, string> = {
  waiting: 'Waiting',
  connected: 'Connected',
  disconnected: 'Disconnected',
};

/** A host's status: neutral while it waits to enroll, success when its agent
 *  reports, danger once the agent goes quiet. Only the danger dot pulses, and
 *  Kiso stops the pulse under prefers-reduced-motion. */
export function HostStatusBadge({ status }: { status: HostStatus }) {
  return (
    <StatusBadge tone={tones[status]} pulse={status === 'disconnected'}>
      {labels[status]}
    </StatusBadge>
  );
}
