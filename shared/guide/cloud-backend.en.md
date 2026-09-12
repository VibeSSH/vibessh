---
id: cloud-backend
title: Account backend
section: getting-started
route: /settings
order: 155
---

Accounts, teams and shared servers need a backend - a separate program that
holds users and permissions. Everything else in VibeSSH works without it:
nodes, applications, files, the terminal, the firewall.

**VibeSSH hosts one, and a fresh install already points at it** — the address
in **Settings** reads `https://api.vibessh.dev` and registering works without
you doing anything. The rest of this page is for the other case: running the
backend yourself, so that your accounts and your teams sit on a server you
own.

If you installed VibeSSH before there was a hosted backend, the setting on
your machine still says `http://localhost:8787` — the old default, meaning a
server on your own computer that is not there. Registering then fails with
"couldn't reach the VibeSSH cloud backend". Replace it with the address above,
or with your own.

## When you actually need it

You need it if you want **accounts and teams** - several people working on the
same servers, with permissions between them.

You do not need it if you use VibeSSH on your own. Adding nodes, creating
applications, files and the terminal all work with no backend and no sign-in.

## Running your own

The backend lives in the `apps/backend/` directory of the VibeSSH source. The
simplest way to run it is Docker - on the same server as your nodes, or on any
other machine.

1. Copy the `apps/backend/` directory onto the server.
2. Copy `.env.example` to `.env` and fill in the **two things** without which
   nothing will start:
   - **POSTGRES_PASSWORD** - the database password, anything long.
   - **JWT_SECRET** - signs the sign-in tokens. Generate a real one, e.g.
     `openssl rand -base64 48`. Leaving the example text there means a secret
     known to everybody who has seen this source.
3. In that directory, run:

```
docker compose up -d
```

4. Check that it answers. From the same server:

```
curl http://localhost:8787/health
```

5. Open port `8787` if the app will reach it from elsewhere. See the
   **Firewall** topic.

The backend keeps its data in PostgreSQL, in a Docker volume called
`vibessh-postgres-data`. Back it up with `docker compose exec postgres pg_dump
-U vibessh_app vibessh > backup.sql` - copying the directory out from under a
running database gives you a file that may not restore.

## Pointing the app at it

1. Open **Settings**.
2. Find the **Account backend** card.
3. In **Backend address**, enter the full address including the scheme, e.g.
   `https://accounts.example.com` or `http://94.130.201.103:8787`.
4. Press **Save**.

While the field still holds the default, the card shows a red warning. It goes
away once you enter your own.

The address is stored on this computer, so **everybody on the team enters it
on their own machine** - otherwise they see the same error.

## Security

If the backend is reachable from the internet, put it behind HTTPS. Over
`http://`, passwords and session tokens cross the network in the clear - the
same network somebody is using to manage servers with root.

The simplest route is nginx or Caddy in front of it with a Let's Encrypt
certificate, and an `https://` address in VibeSSH. On a local network, or
through an SSH tunnel, `http://` is fine.

## Common problems

**"couldn't reach the VibeSSH cloud backend ... this is the development
default".** The address has not been changed yet. See above.

**The address is set and it still does not answer.** Check in order: that the
backend is running (`docker compose ps`), that the port is open in the node's
firewall, and that the address has a scheme - `example.com` on its own will
not work.

**It works for you and not for someone else on the team.** The address is a
local setting on each install. They have to enter it on their machine.
