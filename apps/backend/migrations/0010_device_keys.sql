-- The public half of each device's SSH key, so an install that can already
-- reach a shared Node can install a teammate's key on it.
--
-- Public keys only, and that is the whole point. A member's private key is
-- generated on their own machine and stays in that machine's OS keyring;
-- this backend never sees it, exactly as it never sees an SSH password or a
-- key passphrase (see 0006_team_servers.sql). Publishing a public key is not
-- a secret operation - it is what `authorized_keys` files hold in plain text
-- on every server in the world.
--
-- One row per device, not per user. Somebody with a laptop and a desktop has
-- two keys, and revoking the laptop must not revoke the desktop: that is the
-- behaviour people expect from every other tool that does this, and the
-- opposite would quietly punish having two machines.
CREATE TABLE device_keys (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The full `ssh-ed25519 AAAA... comment` line, as it would appear in an
    -- authorized_keys file. Stored whole rather than parsed so that what is
    -- installed on a Node is byte-for-byte what the device published.
    public_key  TEXT NOT NULL,
    -- What the person calls this machine, shown when they revoke one.
    label       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    -- Updated when a device publishes the same key again, which is how an
    -- install says "still mine" without creating duplicates.
    seen_at     TIMESTAMPTZ NOT NULL
);

CREATE INDEX device_keys_user_id_idx ON device_keys (user_id);

-- The same key twice for one user is one device that re-registered, not two.
CREATE UNIQUE INDEX device_keys_user_key_idx ON device_keys (user_id, public_key);
