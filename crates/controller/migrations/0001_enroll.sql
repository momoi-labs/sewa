-- Hosts, how they enroll, and what their agents report.

CREATE TABLE hosts (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL UNIQUE CHECK (name <> ''),
    os text NOT NULL CHECK (os IN ('linux', 'macos')),
    created_at timestamptz NOT NULL DEFAULT now(),
    -- When the host traded its install token for a credential. NULL means
    -- it never enrolled: its status is `waiting`.
    enrolled_at timestamptz,
    -- When the controller last heard from the agent over OpAMP, by the
    -- controller's clock. The status comes from it.
    last_seen_at timestamptz,
    -- The last connection event recorded for the host is host.connected.
    -- It only decides when to record the next one: the status the API
    -- shows comes from last_seen_at.
    connected boolean NOT NULL DEFAULT false,
    -- What the agent last described. A message without a description keeps
    -- these values.
    agent_instance_uid uuid,
    agent_version text,
    agent_hostname text,
    agent_os text,
    agent_arch text,
    agent_user text
);

-- Single use, and valid for one hour. Only the SHA-256 of the token is kept.
CREATE TABLE install_tokens (
    token_hash bytea PRIMARY KEY,
    host_id uuid NOT NULL REFERENCES hosts (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    used_at timestamptz
);

CREATE INDEX install_tokens_host ON install_tokens (host_id);

-- The credential an agent calls the controller with. Only its SHA-256 is
-- kept. A revoked credential stays as a record and is refused.
CREATE TABLE agent_credentials (
    credential_hash bytea PRIMARY KEY,
    host_id uuid NOT NULL REFERENCES hosts (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);

CREATE INDEX agent_credentials_host ON agent_credentials (host_id);

-- What happened to each host, append-only.
CREATE TABLE events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    host_id uuid NOT NULL REFERENCES hosts (id),
    kind text NOT NULL CHECK (kind IN (
        'host.created', 'host.enrolled', 'host.connected', 'host.disconnected'
    )),
    at timestamptz NOT NULL DEFAULT now(),
    data jsonb NOT NULL DEFAULT '{}'
);

CREATE INDEX events_host ON events (host_id, id);

CREATE FUNCTION events_append_only() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'events are append-only';
END;
$$;

CREATE TRIGGER events_append_only
BEFORE UPDATE OR DELETE OR TRUNCATE ON events
FOR EACH STATEMENT EXECUTE FUNCTION events_append_only();
