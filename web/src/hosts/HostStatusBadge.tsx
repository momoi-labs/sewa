import { StatusBadge, type StatusTone } from '@momoi-labs/kiso-react';

import type { HostStatus } from '../api';

const tones: Record<HostStatus, StatusTone> = {
  waiting: 'neutral',
  connected: 'success',
  disconnected: 'danger',
  disabled: 'neutral',
};

const labels: Record<HostStatus, string> = {
  waiting: 'Waiting',
  connected: 'Connected',
  disconnected: 'Disconnected',
  disabled: 'Disabled',
};

export function HostStatusBadge({ status }: { status: HostStatus }) {
  return (
    <StatusBadge tone={tones[status]} pulse={status === 'disconnected'}>
      {labels[status]}
    </StatusBadge>
  );
}
