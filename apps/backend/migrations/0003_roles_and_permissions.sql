-- Roles + Permissions. Relational, not a bitmask (see permissions.rs) -
-- role_permissions.permission_key is a plain string validated in
-- application code against the catalog there, not a foreign key into a
-- `permissions` table (permission keys live in code, not the database, so
-- the catalog can't silently drift from what the backend actually checks).
--
-- Roles are team-scoped: every team gets its own "Owner" role (seeded in
-- application code when the team is created - see teams.rs::create_team -
-- not here, consistent with how the owner's own team_members row is
-- already created in application code rather than a DB trigger). Custom
-- roles a team creates are ordinary rows here too, distinguished only by
-- is_system = false.

CREATE TABLE roles (
    id          UUID PRIMARY KEY,
    team_id     UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    is_system   BOOLEAN NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    UNIQUE (team_id, name)
);

CREATE TABLE role_permissions (
    role_id        UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_key TEXT NOT NULL,
    PRIMARY KEY (role_id, permission_key)
);

-- References team_members' own composite primary key, not just users(id) -
-- a role can only ever be assigned to someone who is actually a member of
-- that same team; the database itself refuses a role assignment for a
-- non-member rather than relying on application code to remember to check.
CREATE TABLE member_roles (
    team_id UUID NOT NULL,
    user_id UUID NOT NULL,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, user_id, role_id),
    FOREIGN KEY (team_id, user_id) REFERENCES team_members(team_id, user_id) ON DELETE CASCADE
);
