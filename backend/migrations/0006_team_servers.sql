-- Team-scoped server *metadata* - deliberately named team_servers, not
-- servers, to keep it visually distinct from the desktop app's own local
-- SQLite `servers` table (a completely separate database on each device).
-- No password/private-key-path/passphrase columns here on purpose: secrets
-- stay exactly where they already live - the OS keyring on whichever
-- device originally has them - matching the production roadmap's Domain &
-- Database Architecture decision (backend holds metadata for team
-- visibility, never credentials, for this stage). A team member on a
-- different device seeing a shared server here can't yet connect to it
-- with someone else's credentials - that's real future work (shared
-- credential access), not silently faked here.
CREATE TABLE team_servers (
    id         UUID PRIMARY KEY,
    team_id    UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    host       TEXT NOT NULL,
    ssh_port   INTEGER NOT NULL DEFAULT 22,
    username   TEXT,
    added_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX team_servers_team_id_idx ON team_servers (team_id);
