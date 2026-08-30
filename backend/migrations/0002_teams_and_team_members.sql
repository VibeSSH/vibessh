-- Teams + Team Members. Deliberately minimal: no role/permission column on
-- team_members yet - that's the next stage (Roles), which introduces a real
-- roles/permissions/member_roles model rather than a throwaway text column
-- here that would just get migrated away. Ownership for now is a single
-- `owner_id` on `teams` (the owner is also always a team_members row - see
-- backend/src/teams.rs's create_team, which inserts both in one
-- transaction); a real ownership-transfer flow is future work once Roles
-- exists to define what "owner" even grants beyond member-management.

CREATE TABLE teams (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL,
    owner_id   UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE team_members (
    team_id   UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    user_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (team_id, user_id)
);

CREATE INDEX team_members_user_id_idx ON team_members (user_id);
