# Migrations

`0001_users_and_refresh_tokens.sql` is the accounts stage: `users` +
`refresh_tokens`. `teams`, `team_members`, `roles`, `permissions`, and the
rest of the production roadmap's Domain & Database Architecture schema land
in later migrations as their stages start.

New migrations go here as `sqlx migrate add <name>` would generate them:
`<timestamp>_<name>.sql`, applied in filename order via `sqlx::migrate!()` in
`src/main.rs`. Never edit an already-applied migration - a database that has
already recorded it as applied won't re-run it, so an edit only affects
future/fresh databases and silently diverges from ones that already migrated.
