-- The desired state: roles, the roles of each host, and each host's
-- adjustments on top of them. These tables hold only what was approved.
-- Edits wait in a draft change until then.

CREATE TABLE roles (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL UNIQUE CHECK (name <> ''),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- An item is a system package or a mise tool. A package's name is its mise
-- bootstrap key, such as `jq` or `pacman:jq`. Its version `absent` means the
-- package must not be installed.
CREATE TABLE role_items (
    role_id uuid NOT NULL REFERENCES roles (id),
    kind text NOT NULL CHECK (kind IN ('package', 'tool')),
    name text NOT NULL CHECK (name <> ''),
    version text NOT NULL CHECK (version <> ''),
    PRIMARY KEY (role_id, kind, name)
);

CREATE TABLE host_roles (
    host_id uuid NOT NULL REFERENCES hosts (id),
    role_id uuid NOT NULL REFERENCES roles (id),
    PRIMARY KEY (host_id, role_id)
);

-- `set` adds an item or changes its version for this host only. `remove`
-- drops an item that a role of the host declares.
CREATE TABLE host_adjustments (
    host_id uuid NOT NULL REFERENCES hosts (id),
    kind text NOT NULL CHECK (kind IN ('package', 'tool')),
    name text NOT NULL CHECK (name <> ''),
    action text NOT NULL CHECK (action IN ('set', 'remove')),
    version text CHECK ((action = 'set') = (version IS NOT NULL AND version <> '')),
    PRIMARY KEY (host_id, kind, name)
);

-- A change holds edits. It stays a draft until it is approved or
-- discarded. Only one draft exists at a time, so every edit has one place
-- to go.
CREATE TABLE changes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    status text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'approved', 'discarded')),
    created_at timestamptz NOT NULL DEFAULT now(),
    created_by text NOT NULL,
    closed_at timestamptz,
    CHECK ((status = 'draft') = (closed_at IS NULL))
);

CREATE UNIQUE INDEX changes_one_draft ON changes ((true)) WHERE status = 'draft';

CREATE TABLE change_edits (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    change_id uuid NOT NULL REFERENCES changes (id),
    at timestamptz NOT NULL DEFAULT now(),
    by text NOT NULL,
    edit jsonb NOT NULL
);

CREATE INDEX change_edits_change ON change_edits (change_id, id);

-- The last plan of each config the controller sent to a host: `current`
-- for the desired state, or `change/<id>` for a draft change.
CREATE TABLE host_plans (
    host_id uuid NOT NULL REFERENCES hosts (id),
    config text NOT NULL,
    change_id uuid REFERENCES changes (id),
    config_hash text NOT NULL,
    plan_hash text NOT NULL,
    exit_code integer NOT NULL,
    plan jsonb,
    error text,
    reported_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (host_id, config)
);

ALTER TABLE hosts
    -- The packages the host's package manager lists as explicitly
    -- installed, as the agent last reported them.
    ADD COLUMN explicit_manager text,
    ADD COLUMN explicit_packages text[],
    ADD COLUMN explicit_reported_at timestamptz,
    -- The plan hash of the drift last recorded as drift.detected. NULL when
    -- the host was in sync at its last check, so the next drift is new.
    ADD COLUMN drift_plan_hash text,
    -- The health status the agent last reported, such as `in_sync`.
    ADD COLUMN health text;

-- Events now also tell what happened to roles and changes, so an event
-- may name no host.
ALTER TABLE events
    ALTER COLUMN host_id DROP NOT NULL,
    ADD COLUMN change_id uuid REFERENCES changes (id),
    ADD COLUMN role_id uuid REFERENCES roles (id),
    DROP CONSTRAINT events_kind_check,
    ADD CONSTRAINT events_kind_check CHECK (kind IN (
        'host.created', 'host.enrolled', 'host.connected', 'host.disconnected',
        'role.created', 'change.created', 'change.edited', 'change.discarded',
        'drift.detected'
    ));

CREATE INDEX events_change ON events (change_id, id) WHERE change_id IS NOT NULL;
