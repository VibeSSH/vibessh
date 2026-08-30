-- Invitations. Tokens are opaque and random (see tokens.rs), hashed at
-- rest exactly like refresh_tokens - the raw value is handed to the caller
-- exactly once, in the create-invitation response, and never persisted or
-- retrievable again. This backend has no email-delivery integration, so
-- actually getting that raw token to the invitee (email, Slack, a copied
-- link) is a client concern, not something this migration or the
-- invitations.rs endpoints assume or hardcode a URL for.
--
-- `status` only ever stores 'pending', 'accepted', 'revoked', or
-- 'declined' - "expired" is deliberately not a stored status (same
-- reasoning as refresh_tokens): whether a pending invitation has expired is
-- computed from `expires_at` at read time, not written by a background job.
--
-- role_id is ON DELETE SET NULL, not CASCADE - a role being deleted after
-- an invitation referencing it was sent shouldn't destroy the invitation
-- itself; accepting it then just adds the member with no role assigned
-- instead of failing.
CREATE TABLE invitations (
    id          UUID PRIMARY KEY,
    team_id     UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    email       TEXT NOT NULL,
    role_id     UUID REFERENCES roles(id) ON DELETE SET NULL,
    token_hash  TEXT NOT NULL UNIQUE,
    status      TEXT NOT NULL DEFAULT 'pending',
    invited_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ
);

-- At most one pending invitation per (team, email) at a time - a real
-- database-level guarantee against duplicate concurrent invites, not just
-- an application-level check-then-insert that a race could slip past.
CREATE UNIQUE INDEX invitations_team_email_pending_idx ON invitations (team_id, email) WHERE status = 'pending';
CREATE INDEX invitations_team_id_idx ON invitations (team_id);
