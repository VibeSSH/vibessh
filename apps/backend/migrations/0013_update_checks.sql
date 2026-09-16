-- How many people are running VibeSSH, without keeping who they are.
--
-- The app already asks for an update manifest every six hours, and until now
-- it asked GitHub - so the only party who could count installations was
-- Microsoft, and the answer they give is a download counter that cannot tell
-- one machine left running from forty machines opened once.
--
-- **What this deliberately does not store.** No address, and no identifier
-- that survives the day. `client_day_hash` is a hash of the caller's address
-- together with a salt that changes every day, so two checks from the same
-- machine collapse into one row *within* a day and cannot be connected
-- *across* days. That buys the one number worth having - how many distinct
-- installations ran today - and refuses the one that would make this
-- tracking: a profile of a machine over time.
--
-- The cost is deliberate and worth naming: retention cannot be measured from
-- this table. Answering "did last week's users come back" needs an
-- identifier that outlives a day, which is exactly what is not here.
CREATE TABLE update_checks (
    day             DATE NOT NULL,
    -- SHA-256 of (daily salt || client address). Not reversible to an
    -- address, and not comparable to yesterday's hash of the same address.
    client_day_hash BYTEA NOT NULL,
    -- What the caller said it is. Free text from the client and treated as
    -- such: it is a label for grouping, never trusted for a decision.
    version         TEXT,
    platform        TEXT,
    checks          INTEGER NOT NULL DEFAULT 1,
    last_seen_at    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (day, client_day_hash, version, platform)
);

CREATE INDEX update_checks_day_idx ON update_checks (day);

-- The salt that makes a day's hashes incomparable to another day's.
--
-- Generated on the first check of each day and never shown. Old rows are
-- kept only so the same day's checks keep colliding into the same row if the
-- backend restarts; a salt whose day has passed has no further use, and
-- deleting it makes that day's hashes permanently unlinkable to any address
-- even by us.
CREATE TABLE update_check_salts (
    day  DATE PRIMARY KEY,
    salt BYTEA NOT NULL
);
