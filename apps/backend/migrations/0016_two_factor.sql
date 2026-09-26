-- Two-factor sign-in with a time-based code - see `src/two_factor.rs`.
--
-- `totp_secret` is the active secret, `totp_pending_secret` one being set up
-- that has not yet been confirmed with a code from the person's app - kept
-- apart so an unfinished setup never locks anybody out. Both are encrypted
-- (AES-256-GCM, `TOTP_ENCRYPTION_KEY`), never stored as the secret itself.
--
-- `totp_last_step` is the time step of the last code accepted, so the same
-- code cannot be used twice.
ALTER TABLE users ADD COLUMN totp_secret BYTEA;
ALTER TABLE users ADD COLUMN totp_pending_secret BYTEA;
ALTER TABLE users ADD COLUMN totp_enabled_at TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN totp_last_step BIGINT;

-- Single-use codes for when the phone is gone. SHA-256 of the normalised
-- code: they are random enough that a slow hash would only slow down the
-- check, which has to look them up.
CREATE TABLE totp_recovery_codes (
    user_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at   TIMESTAMPTZ,
    PRIMARY KEY (user_id, code_hash)
);
