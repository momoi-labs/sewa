-- Reports an agent kept while it could not reach the controller arrive
-- late. They keep the time they happened, by the agent's clock, in
-- reported_at. received_at is when the controller got them.

ALTER TABLE host_plans ADD COLUMN received_at timestamptz;
ALTER TABLE change_hosts ADD COLUMN received_at timestamptz;

-- host.offline: the controller heard nothing from the host for a while,
-- because the host or the controller was down. Its time is when the
-- silence began.
ALTER TABLE events
    DROP CONSTRAINT events_kind_check,
    ADD CONSTRAINT events_kind_check CHECK (kind IN (
        'host.created', 'host.enrolled', 'host.connected', 'host.disconnected',
        'host.offline',
        'role.created', 'change.created', 'change.edited', 'change.discarded',
        'drift.detected', 'passkey.registered', 'change.approved',
        'plan.applied', 'apply.failed', 'apply.replan',
        'drift.reverted', 'drift.adopted', 'drift.ignored'
    ));

-- The events of a host read newest first by the time they happened.
CREATE INDEX events_host_at ON events (host_id, at, id);
