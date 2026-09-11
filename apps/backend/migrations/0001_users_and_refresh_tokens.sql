-- First real schema: accounts. Team/Roles/Permissions/Invitations/Audit
-- tables land in later migrations once the Team stage starts - this one is
-- scoped to exactly what register/login/refresh needs.

CREATE TABLE users (
    id            UUID PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name  TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL
);

-- Refresh tokens are opaque random values, never JWTs - unlike a stateless
-- access token, a refresh token has to be revocable (logout, a stolen
-- device, a rotated-away-from token after use) and that requires a server
-- side record to revoke. Only the SHA-256 hash is stored, never the token
-- itself - identical reasoning to why passwords are hashed, since anyone who
-- reads this table shouldn't be able to use what they find in it.
CREATE TABLE refresh_tokens (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ
);

CREATE INDEX refresh_tokens_user_id_idx ON refresh_tokens (user_id);
