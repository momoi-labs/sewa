-- A host's OS is the one its agent reports, and unknown until it connects.
ALTER TABLE hosts ALTER COLUMN os DROP NOT NULL;
UPDATE hosts SET os = CASE agent_os WHEN 'darwin' THEN 'macos' WHEN 'linux' THEN 'linux' END;
