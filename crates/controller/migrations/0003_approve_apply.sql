-- Approvals: the passkeys that sign them, the approvals themselves, and
-- what each host did with them.

-- A passkey of an admin. Its public key is SubjectPublicKeyInfo DER of an
-- ES256 key. The install script pins every passkey into the agent.
CREATE TABLE passkeys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    login text NOT NULL,
    name text NOT NULL CHECK (name <> ''),
    credential_id bytea NOT NULL UNIQUE,
    public_key bytea NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- The challenge of a passkey registration in progress. Single use, and
-- valid for a few minutes.
CREATE TABLE passkey_challenges (
    challenge bytea PRIMARY KEY,
    login text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- A revert change applies the approved state again on one host, to undo
-- its drift. It has no edits. Each host has at most one open revert, and
-- the one draft of edits stays apart from them.
ALTER TABLE changes ADD COLUMN revert_host uuid REFERENCES hosts (id);

DROP INDEX changes_one_draft;
CREATE UNIQUE INDEX changes_one_draft ON changes ((true))
    WHERE status = 'draft' AND revert_host IS NULL;
CREATE UNIQUE INDEX changes_one_revert ON changes (revert_host)
    WHERE status = 'draft' AND revert_host IS NOT NULL;

-- One signature of a change. `hosts` is the list the challenge binds:
-- host, agent instance, config hash, and plan hash.
CREATE TABLE approvals (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    change_id uuid NOT NULL REFERENCES changes (id),
    by text NOT NULL,
    passkey_id uuid NOT NULL REFERENCES passkeys (id),
    hosts jsonb NOT NULL,
    assertion jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX approvals_change ON approvals (change_id, created_at);

-- Where each host of an approved change stands: `waiting` until its agent
-- reports, then `applied`, `failed`, or `replan` when its plan changed and
-- the change needs a new approval for it.
CREATE TABLE change_hosts (
    change_id uuid NOT NULL REFERENCES changes (id),
    host_id uuid NOT NULL REFERENCES hosts (id),
    approval_id uuid NOT NULL REFERENCES approvals (id),
    plan_hash text NOT NULL,
    status text NOT NULL DEFAULT 'waiting'
        CHECK (status IN ('waiting', 'applied', 'failed', 'replan')),
    log text,
    reported_at timestamptz,
    PRIMARY KEY (change_id, host_id)
);

CREATE INDEX change_hosts_waiting ON change_hosts (host_id) WHERE status = 'waiting';

ALTER TABLE hosts
    -- What the last Ignore hid: the drift plan hash and the undeclared
    -- packages of that moment.
    ADD COLUMN drift_ignored jsonb;

ALTER TABLE events
    DROP CONSTRAINT events_kind_check,
    ADD CONSTRAINT events_kind_check CHECK (kind IN (
        'host.created', 'host.enrolled', 'host.connected', 'host.disconnected',
        'role.created', 'change.created', 'change.edited', 'change.discarded',
        'drift.detected', 'passkey.registered', 'change.approved',
        'plan.applied', 'apply.failed', 'apply.replan',
        'drift.reverted', 'drift.adopted', 'drift.ignored'
    ));
