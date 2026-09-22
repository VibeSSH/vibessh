-- Which team members may see a shared Application.
--
-- **Why this table exists.** `team_applications` is visible to a whole team:
-- any member gets every shared Application. Some Applications should only be
-- seen by specific people, so this is an allow-list layered on top of the
-- share. Its meaning is deliberately opt-in: an Application with no rows here
-- stays visible to the whole team (the behaviour that already shipped), and
-- the moment one row is added the Application becomes visible to exactly the
-- listed members and whoever pushed it. `list_applications` reads it that way.
--
-- **Why the compound foreign key.** `(team_id, user_id)` references
-- `team_members`, mirroring `member_roles`: the database itself refuses a
-- grant to somebody who is not in the team, and removes the grant when they
-- leave. `application_id` cascades from `team_applications`, so unsharing an
-- Application takes its allow-list with it. Nothing has to remember to clean
-- either of these up.
CREATE TABLE application_members (
    application_id UUID NOT NULL REFERENCES team_applications(id) ON DELETE CASCADE,
    team_id        UUID NOT NULL,
    user_id        UUID NOT NULL,
    -- Who granted access, for the audit trail. Nulled rather than cascaded so
    -- the grant survives the granter leaving - it is the member's access that
    -- matters, not who opened the door.
    granted_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    granted_at     TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (application_id, user_id),
    FOREIGN KEY (team_id, user_id) REFERENCES team_members(team_id, user_id) ON DELETE CASCADE
);

-- The filter in `list_applications` asks "who may see this Application", and
-- the tab asks "which Applications may this member see" - one index per
-- direction.
CREATE INDEX application_members_app_idx ON application_members (application_id);
CREATE INDEX application_members_member_idx ON application_members (team_id, user_id);
