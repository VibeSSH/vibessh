# The hosted account backend

`https://api.vibessh.dev` is the backend a fresh VibeSSH install talks to. It
is what `cloud_config::DEFAULT_BACKEND_URL` points at, so accounts, teams and
shared servers work without anybody configuring anything. Self-hosting is
still supported and is what `shared/guide/cloud-backend.*.md` describes; this
file is about the instance we run.

## Where it runs

On the same VPS as the repository's self-hosted GitHub Actions runner. That
was a deliberate decision to run one machine rather than two, taken knowing
the cost: a self-hosted runner executes whatever a workflow says, and this
backend holds account passwords and the secret that signs access tokens. The
isolation that survives is therefore load-bearing and must not be relaxed:

- the runner's user (`ghrunner`) has no sudo,
- it is not in the `docker` group and cannot reach the Docker socket,
- it cannot read the backend's environment file.

Each of those is worth re-checking after any change to the runner setup. They
are the only thing standing between a malicious pull request and the accounts
database.

## Layout on the host

| Path | What it is |
| --- | --- |
| `/opt/vibessh/src` | A checkout of this repository, shipped with `git archive` |
| `/opt/vibessh/secrets/backend.env` | `POSTGRES_PASSWORD`, `JWT_SECRET`, `TOTP_ENCRYPTION_KEY` and the `SMTP_*`/`MAIL_FROM` settings, mode 0600, owned by root |
| `/etc/cloudflared/config.yml` | Tunnel ingress: `api.vibessh.dev` → `127.0.0.1:8787` |

The secrets were generated on the host with `openssl rand` and have never
left it. Rotating `JWT_SECRET` invalidates every currently-issued access
token, which clients recover from by refreshing; see `.env.example`.

`TOTP_ENCRYPTION_KEY` (`openssl rand -base64 32`) encrypts every account's
two-factor secret. Unlike `JWT_SECRET` it must never be rotated or lost on
its own: without the key it was stored under, no account with two-factor on
can sign in. Back it up together with the database.

`SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD` and `MAIL_FROM`
are the mailbox password-reset codes are sent from. Without them the service
runs and a reset is refused with a reason. The sending domain needs the
provider in its SPF record and its DKIM key published, or the codes land in
spam.

## Deploying a change

```bash
git archive --format=tar HEAD | ssh <host> 'tar xf - -C /opt/vibessh/src'
ssh <host> 'cd /opt/vibessh/src/apps/backend \
  && sudo docker compose --env-file /opt/vibessh/secrets/backend.env up -d --build'
```

Migrations are embedded in the binary (`sqlx::migrate!`) and run at startup,
so there is no separate migration step. `curl https://api.vibessh.dev/health`
should answer `{"database":"connected","status":"ok"}`.

## Why there is no web server and no certificate here

The backend binds `127.0.0.1:8787` and is published by a Cloudflare Tunnel,
which dials out. Ports 80 and 443 are closed in ufw; only SSH is open. A
tunnel rather than the orange cloud alone because proxying the DNS record
hides the address only from someone reading DNS - the origin keeps answering
on 443 to anything that scans the address space, and the certificate it
serves names the site. With a tunnel there is nothing listening to find.

The cost, recorded so that it is a known position rather than an oversight:
Cloudflare terminates TLS and therefore sees the plaintext of every sign-in.
Serving the origin directly would avoid that and publish the address instead.

Because the port is bound to loopback, ufw is not what keeps it private -
Docker writes its forwarding rules ahead of ufw's, so a port published on all
interfaces would be reachable regardless of what ufw says. The bind address
in `docker-compose.yml` is the control that matters.
