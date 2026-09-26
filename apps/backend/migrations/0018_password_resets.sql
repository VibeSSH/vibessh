-- Codes for resetting a forgotten password, sent by email (`password_reset`).
--
-- **Only a hash is stored.** The code is in the person's inbox and nowhere
-- else; a database dump yields SHA-256 digests of short-lived, single-use
-- values, which is nothing to sign in with.
--
-- **Short-lived, single-use, and bounded.** `expires_at` is thirty minutes
-- after the request; `used_at` is set the moment one works, in the same
-- statement that checks it was not set already, so two racing requests
-- cannot both spend it; and `attempts` counts wrong guesses against this
-- code, which stops accepting any once it reaches the limit - so the code's
-- length, not the rate limiter alone, is what a guesser is up against.
--
-- A new request retires the older codes of the same account (see the
-- handler), so only the latest one in the inbox works.
CREATE TABLE password_resets (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash  TEXT NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at    TIMESTAMPTZ
);

CREATE INDEX password_resets_user_idx ON password_resets (user_id, created_at DESC);
