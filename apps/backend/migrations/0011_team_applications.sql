-- Applications a team can see, as a projection of the install that owns
-- them.
--
-- **Why this is a projection and not the source of truth.** An Application
-- is created and run by one install, whose local SQLite holds the record it
-- acts on. This table exists so the *other* members can see what is on a
-- shared Node without that install being open. The owning install pushes;
-- nothing here is ever the thing a runtime reads.
--
-- **Why ports and environment are JSONB rather than child tables.** A
-- projection is replaced wholesale, never edited field by field, so nothing
-- here needs to join, filter or index into them. Two child tables would buy
-- a normal form nobody queries and cost a delete-and-reinsert dance on every
-- push. The shape is documented by the Rust types that write it.
--
-- **Secret environment values are not here, by construction.** They are
-- already on the Node, in the Application's own environment file, which is
-- where the process reads them from. A member does not need them to start,
-- stop, inspect or back up an Application - only to recreate one, which is a
-- separate permission and can ask at that moment. See
-- docs/planning/team-access-design.md.
CREATE TABLE team_applications (
    id                UUID PRIMARY KEY,
    team_id           UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    -- Which shared server this runs on. Null for an Application on a machine
    -- the team does not share, which is still worth listing so a member
    -- understands why they cannot reach it.
    team_server_id    UUID REFERENCES team_servers(id) ON DELETE SET NULL,
    -- The id the owning install knows it by. This is what makes a push
    -- idempotent: the same Application pushed twice updates rather than
    -- duplicating, across restarts and reinstalls.
    local_id          UUID NOT NULL,
    name              TEXT NOT NULL,
    blueprint_id      TEXT NOT NULL,
    runtime_type      TEXT NOT NULL,
    working_directory TEXT NOT NULL,
    -- `[{name, protocol, internalPort, externalPort, visibility}]`
    ports             JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- `[{key, value}]` - non-secret only; see the note above.
    environment       JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Who pushed it last, and when. A projection that is three weeks old is
    -- worth knowing about, and without this nobody could tell.
    pushed_by         UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL
);

CREATE INDEX team_applications_team_id_idx ON team_applications (team_id);

-- One row per Application per team. Pushing the same one again updates it.
CREATE UNIQUE INDEX team_applications_team_local_idx ON team_applications (team_id, local_id);
