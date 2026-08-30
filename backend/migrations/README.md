# Migrations

Empty for now - this stage is the backend scaffold only (connection, migration
runner, health check). The first real migration (`users`, `teams`,
`team_members`, ...) lands in the next stage, designed against the schema in
the production roadmap's Domain & Database Architecture section.

New migrations go here as `sqlx migrate add <name>` would generate them:
`<timestamp>_<name>.sql`, applied in filename order via `sqlx::migrate!()` in
`src/main.rs`. Never edit an already-applied migration - a database that has
already recorded it as applied won't re-run it, so an edit only affects
future/fresh databases and silently diverges from ones that already migrated.
