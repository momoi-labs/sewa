ALTER TABLE sync_hosts
    DROP CONSTRAINT sync_hosts_status_check,
    DROP CONSTRAINT sync_hosts_check,
    ADD CONSTRAINT sync_hosts_status_check CHECK (status IN (
        'picked', 'in_sync', 'skipped', 'waiting', 'applying', 'applied', 'failed', 'replan'
    )),
    ADD CONSTRAINT sync_hosts_check CHECK (
        (status IN ('waiting', 'applying', 'applied', 'failed', 'replan')) = (approval_id IS NOT NULL)
    ),
    ADD COLUMN step text,
    ADD COLUMN started_at timestamptz,
    ADD COLUMN progress_at timestamptz,
    ADD COLUMN steps jsonb NOT NULL DEFAULT '[]';

DROP INDEX sync_hosts_waiting;

CREATE INDEX sync_hosts_waiting ON sync_hosts (host_id) WHERE status IN ('waiting', 'applying');
