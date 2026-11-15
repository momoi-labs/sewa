-- No drafts. Roles and the roles of each host are edited directly, and a
-- sync applies the desired state to the hosts an admin picks. See
-- docs/adr/0001-direct-edits-and-sync.md.
--
-- The draft model's data goes: changes, their edits, their approvals, and
-- the plans of their configs. Events keep their history, without the link
-- to a change.

ALTER TABLE events DROP COLUMN change_id;

DELETE FROM host_plans WHERE config <> 'current';
ALTER TABLE host_plans DROP COLUMN change_id;

DROP TABLE change_hosts;
DROP TABLE approvals;
DROP TABLE change_edits;
DROP TABLE changes;

-- The built-in role holds what sewa itself needs on a host, such as mise.
-- It applies to every host, and it cannot be deleted or renamed. The
-- controller keeps its name in step with its constant at start.
ALTER TABLE roles ADD COLUMN builtin boolean NOT NULL DEFAULT false;
CREATE UNIQUE INDEX roles_one_builtin ON roles ((true)) WHERE builtin;

INSERT INTO roles (name, builtin) VALUES ('bootstrap', true)
    ON CONFLICT (name) DO UPDATE SET builtin = true;
INSERT INTO role_items (role_id, kind, name, version)
    SELECT id, 'package', 'mise', 'latest' FROM roles WHERE builtin
    ON CONFLICT DO NOTHING;

-- An item for one OS only. NULL means every OS.
ALTER TABLE role_items ADD COLUMN os text CHECK (os IN ('linux', 'macos'));

-- A sync: the hosts an admin picked to bring to the desired state.
CREATE TABLE syncs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at timestamptz NOT NULL DEFAULT now(),
    created_by text NOT NULL
);

-- One signature of a sync. `hosts` is the list the challenge binds: host,
-- agent instance, config hash, and plan hash.
CREATE TABLE approvals (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    sync_id uuid NOT NULL REFERENCES syncs (id),
    by text NOT NULL,
    passkey_id uuid NOT NULL REFERENCES passkeys (id),
    hosts jsonb NOT NULL,
    assertion jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX approvals_sync ON approvals (sync_id, created_at);

-- Where each picked host stands: `picked` until a signature, then
-- `in_sync` (nothing to do), `skipped` (no plan to sign), or `waiting`
-- until its agent reports `applied`, `failed`, or `replan`.
CREATE TABLE sync_hosts (
    sync_id uuid NOT NULL REFERENCES syncs (id),
    host_id uuid NOT NULL REFERENCES hosts (id),
    status text NOT NULL DEFAULT 'picked' CHECK (status IN (
        'picked', 'in_sync', 'skipped', 'waiting', 'applied', 'failed', 'replan'
    )),
    approval_id uuid REFERENCES approvals (id),
    plan_hash text,
    log text,
    reported_at timestamptz,
    received_at timestamptz,
    CHECK ((status IN ('waiting', 'applied', 'failed', 'replan')) = (approval_id IS NOT NULL)),
    PRIMARY KEY (sync_id, host_id)
);

CREATE INDEX sync_hosts_waiting ON sync_hosts (host_id) WHERE status = 'waiting';

ALTER TABLE events
    ADD COLUMN sync_id uuid REFERENCES syncs (id),
    DROP CONSTRAINT events_kind_check,
    ADD CONSTRAINT events_kind_check CHECK (kind IN (
        'host.created', 'host.enrolled', 'host.connected', 'host.disconnected',
        'host.offline',
        'role.created', 'change.created', 'change.edited', 'change.discarded',
        'drift.detected', 'passkey.registered', 'change.approved',
        'plan.applied', 'apply.failed', 'apply.replan',
        'drift.reverted', 'drift.adopted', 'drift.ignored',
        'role.saved', 'host.roles_set', 'host.items_set',
        'sync.created', 'sync.approved'
    ));

CREATE INDEX events_sync ON events (sync_id, id) WHERE sync_id IS NOT NULL;
