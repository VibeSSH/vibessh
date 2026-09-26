-- What a member may do with one shared Application, not only whether they see it.
--
-- **Why here and not in a role.** A role is team-wide: whatever it grants, it
-- grants on every Application the team shares. Handing somebody "restart"
-- on one Minecraft server and nothing on the next one - what Pterodactyl
-- calls a subuser - needs the grant to sit on the Application. This table
-- already says who may see it; a row now also says what they may do there.
--
-- **Only the permissions a Node can hold to one Application.** Starting and
-- stopping its container, writing to its console, and reading or writing its
-- files as its own account are commands a sudo rule can name for exactly one
-- Application. Everything else in the catalog either reaches every
-- Application at once or is root on the machine, so it stays in roles, where
-- the interface already says so. The CHECK keeps anything else from being
-- stored, whatever a client sends.
--
-- Empty - the default, and every row that existed before this - means "can
-- see it", which is what those rows meant when they were written.
ALTER TABLE application_members
    ADD COLUMN permissions TEXT[] NOT NULL DEFAULT '{}'
    CHECK (permissions <@ ARRAY[
        'applications.lifecycle',
        'applications.console',
        'applications.files.read',
        'applications.files.write'
    ]::TEXT[]);
