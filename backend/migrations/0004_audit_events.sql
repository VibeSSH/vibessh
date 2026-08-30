-- Audit Log. Append-only in practice (no application code ever issues an
-- UPDATE or DELETE against this table - only INSERT and SELECT) - a
-- database-level guarantee (revoking UPDATE/DELETE grants from the app
-- role) is worth adding once a real ops/migration process exists to manage
-- role grants outside of migrations; noting it here rather than silently
-- skipping it.
--
-- actor_id is nullable with ON DELETE SET NULL, not CASCADE - deleting a
-- user must not erase the historical record of what they did (Data
-- Integrity: "must not accidentally delete audit history"). team_id is
-- ON DELETE CASCADE, a deliberate scope limit for now: this app has no
-- soft-delete/archival story for teams yet, so a hard-deleted team's audit
-- trail goes with it rather than existing as orphaned unreadable rows -
-- worth revisiting if/when team deletion needs to preserve history.
CREATE TABLE audit_events (
    id          UUID PRIMARY KEY,
    team_id     UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    actor_id    UUID REFERENCES users(id) ON DELETE SET NULL,
    action      TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id   UUID,
    -- Always 'success' for now - only successful mutations are recorded in
    -- this stage. Logging denied/failed attempts (permission-denied audit
    -- events, failed logins) is real future work, not done here; the
    -- column exists now so that work is an application-code change, not
    -- another migration.
    result      TEXT NOT NULL DEFAULT 'success',
    metadata    JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX audit_events_team_id_created_at_idx ON audit_events (team_id, created_at DESC);
