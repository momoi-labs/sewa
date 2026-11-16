
ALTER TABLE hosts ADD COLUMN disabled_at timestamptz;

ALTER TABLE events
    DROP CONSTRAINT events_kind_check,
    ADD CONSTRAINT events_kind_check CHECK (kind IN (
        'host.created', 'host.enrolled', 'host.connected', 'host.disconnected',
        'host.offline',
        'role.created', 'change.created', 'change.edited', 'change.discarded',
        'drift.detected', 'passkey.registered', 'change.approved',
        'plan.applied', 'apply.failed', 'apply.replan',
        'drift.reverted', 'drift.adopted', 'drift.ignored',
        'role.saved', 'host.roles_set', 'host.items_set',
        'sync.created', 'sync.approved',
        'host.disabled', 'host.enabled'
    ));

CREATE OR REPLACE FUNCTION events_append_only() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND current_setting('sewa.deleting_host', true) = 'on' THEN
        RETURN NULL;
    END IF;
    RAISE EXCEPTION 'events are append-only';
END;
$$;
