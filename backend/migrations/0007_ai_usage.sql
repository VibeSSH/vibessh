-- Per-account daily usage of the hosted Vibe AI assistant.
--
-- This table is the whole reason the hosted assistant proxies through the
-- backend at all. VibeSSH's own Qwen key pays for every question asked
-- through the built-in default, so "one account cannot spend the shared
-- allowance" has to be enforced somewhere the account holder does not
-- control. A counter in the desktop app is not that place: the binary is
-- inspectable and the counter is editable, and if the key were shipped to
-- the client it would not need the counter bypassed at all - it could be
-- extracted and used directly.
--
-- One row per (user, day) rather than an event log. A log would allow a
-- rolling 24h window, which is fairer across a midnight boundary, but it
-- grows without bound and it cannot be explained to a user in one sentence.
-- "20 questions a day, resets at midnight UTC" can be, and the UI can show
-- it as a number. If a rolling window is wanted later it is a new table
-- rather than a change to this one.
--
-- `usage_date` is a DATE in UTC, which is what CURRENT_DATE gives on a
-- server configured to UTC. That is deliberate rather than per-user local
-- time: local time would need a stored timezone per account and would make
-- the reset moment un-explainable to a team spread across zones.
CREATE TABLE ai_usage (
    user_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    usage_date     DATE NOT NULL,
    -- Questions asked. Incremented before the upstream call and decremented
    -- again if that call fails, so a provider outage does not silently eat
    -- somebody's allowance - see `ai::chat`.
    question_count INTEGER NOT NULL DEFAULT 0,
    -- Rough cost signal. Not enforced as a limit in this version: the
    -- question count is what the user is told about, and two limits where
    -- one is invisible produces a refusal nobody can explain. Recorded from
    -- the start because it is the number that would justify changing the
    -- limit later, and it cannot be backfilled.
    prompt_chars   BIGINT NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, usage_date)
);

-- Yesterday's rows are never read again once the day turns, so the only
-- access pattern is "this user, today". The primary key already serves it;
-- this index exists for the cleanup path a future retention job would use.
CREATE INDEX ai_usage_by_date ON ai_usage (usage_date);
