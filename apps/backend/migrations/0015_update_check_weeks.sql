-- How many distinct installations ran in a week, and what that costs.
--
-- 0013 kept no identifier that outlives a day, so the only count it could
-- give was one day's. A day under-counts an app nobody opens daily: on the
-- day it was asked for, "active installations" said 2 while the installers
-- had been downloaded 99 times, and neither number said how many people use
-- VibeSSH. A week is the shortest window that does.
--
-- **What this gives up, stated plainly.** `client_week_hash` makes the same
-- machine (strictly: the same address) one row per calendar week instead of
-- per day, so within a week its checks can be linked to each other. Nothing
-- beyond that: the hash is of the address with a salt that exists only for
-- that week, the address itself is still never stored, and the salt is
-- deleted once its week has ended - after which that week's hashes cannot be
-- matched to an address even by us, exactly as 0013's daily ones already
-- could not. Weeks cannot be linked to one another, so retention is still
-- not measurable, on purpose.
--
-- Calendar weeks (Monday to Sunday, UTC) rather than a rolling seven days,
-- because a rolling window would need one identifier to span every pair of
-- days seven apart, which is an identifier that never expires.
ALTER TABLE update_checks ADD COLUMN client_week_hash BYTEA;

-- Checks recorded before this column existed have no week hash. A week that
-- contains any of them is incomplete, and is reported as unknown rather than
-- as the smaller number it would add up to.

CREATE TABLE update_check_week_salts (
    -- The Monday the week starts on.
    week_start DATE PRIMARY KEY,
    salt       BYTEA NOT NULL
);
