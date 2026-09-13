-- What still has to be taken off a Node, and has not been yet.
--
-- Removing somebody from a team deletes a row here in the backend. Their
-- account on each shared Node is a different thing in a different place, and
-- only an install that can reach that Node over SSH can remove it. The two
-- cannot be done in one operation, and the gap between them is not a
-- technicality: it is the difference between "this person no longer has
-- access" and "we asked for their access to be removed", which is the entire
-- point of the feature.
--
-- So the intent is recorded here, and any install with access to the Node
-- completes it - the same reconcile shape `firewall_service` uses on the
-- desktop. Until one does, the interface shows the revocation as pending,
-- because a list that showed it as done would be the worst possible thing on
-- this screen to be wrong about.
--
-- See docs/planning/team-access-design.md, "Revocation".
CREATE TABLE node_revocations (
    id             UUID PRIMARY KEY,
    team_id        UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    -- Which Node. Cascades: a team server that is no longer shared has no
    -- account left to think about, and a pending row pointing at nothing
    -- would be a task nobody could ever complete.
    team_server_id UUID NOT NULL REFERENCES team_servers(id) ON DELETE CASCADE,
    -- Deliberately not a foreign key to users. This row outlives the
    -- membership it came from, and has to outlive the user record too: an
    -- account still sitting on somebody's server is exactly as real after
    -- the user row is gone, and a cascade here would quietly erase the one
    -- record saying so.
    user_id        UUID NOT NULL,
    -- The Linux account to remove, and the address to name in the list.
    -- Copied rather than derived on read for the same reason: they must
    -- still be readable when there is no user row left to derive them from.
    node_username  TEXT NOT NULL,
    email          TEXT NOT NULL,
    requested_at   TIMESTAMPTZ NOT NULL,
    requested_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    -- NULL is the pending state. Rows are kept once completed rather than
    -- deleted, so "when did this actually come off the machine" has an
    -- answer later, not just at the moment it happened.
    completed_at   TIMESTAMPTZ,
    completed_by   UUID REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX node_revocations_team_idx ON node_revocations (team_id);

-- One outstanding revocation per person per Node. Somebody removed, re-added
-- and removed again before anybody synced has one thing owed, not two; once
-- the first has landed, the second is a new and real one, which is why the
-- index is partial rather than absolute.
CREATE UNIQUE INDEX node_revocations_pending_idx
    ON node_revocations (team_server_id, user_id)
    WHERE completed_at IS NULL;
