#!/bin/sh
# Put what is on `main` onto this host, in one command.
#
#   sudo sh /opt/vibessh/src/apps/backend/scripts/deploy.sh
#   sudo sh /opt/vibessh/src/apps/backend/scripts/deploy.sh v0.1.0-beta.14
#
# Before this existed a deploy meant copying changed files up by hand and
# then remembering that `docker compose` needs `--env-file`, because the
# secrets live outside the tree. Forgetting the second half fails loudly and
# harmlessly; forgetting a file in the first half deploys something that
# exists on no machine but this one. So the source is now a clone, the ref is
# named, and neither step is retyped.
#
# **First time on a host**, where `/opt/vibessh/src` is a plain directory:
#
#   cd /opt/vibessh/src
#   sudo git init -q
#   sudo git remote add origin https://github.com/VibeSSH/vibessh.git
#   sudo git fetch -q origin
#   sudo git reset --hard origin/main
#
# `git init` inside the existing directory rather than a fresh clone, so the
# path, the compose project and the running containers stay exactly where
# they are. `reset --hard` overwrites tracked files and leaves untracked ones
# alone, so anything genuinely local to this host survives it.
#
# Everything here is idempotent: run it again and it backs up again, fetches
# again and rebuilds whatever changed.
set -eu

# --- this script updates itself half-way through ---------------------
#
# `git reset --hard` can rewrite this very file while the shell is still
# reading it, and a shell whose script changed under it carries on at the old
# byte offset - which is now the middle of a different line. Running from a
# copy costs nothing and removes the whole class of problem.
if [ "${VIBESSH_DEPLOY_REEXEC:-}" != "1" ]; then
    copy=$(mktemp)
    cat "$0" > "$copy"
    VIBESSH_DEPLOY_REEXEC=1 sh "$copy" "$@" || status=$?
    rm -f "$copy"
    exit "${status:-0}"
fi

SRC=${VIBESSH_SRC:-/opt/vibessh/src}
ENV_FILE=${VIBESSH_ENV_FILE:-/opt/vibessh/secrets/backend.env}
BACKUPS=${VIBESSH_BACKUPS:-/opt/vibessh/backups}
LOG=${VIBESSH_DEPLOY_LOG:-/opt/vibessh/deploy.log}
# Loopback, because that is where the compose file publishes it - see the
# comment on `ports` for why it is not on every interface.
HEALTH_URL=${VIBESSH_HEALTH_URL:-http://127.0.0.1:8787/health}
HEALTH_TIMEOUT=${VIBESSH_HEALTH_TIMEOUT:-120}
KEEP_BACKUPS=${VIBESSH_KEEP_BACKUPS:-30}
REF=${1:-origin/main}

COMPOSE_DIR="$SRC/apps/backend"
# Read as root over a tree owned by somebody else, which git refuses by
# default. Passed per-command rather than written into a global config, so
# running this leaves no setting behind on the host.
git_() { git -C "$SRC" -c "safe.directory=$SRC" "$@"; }

say() { printf '\n== %s\n' "$*"; }
die() { printf '\ndeploy failed: %s\n' "$*" >&2; exit 1; }

# --- everything that must be true before anything is touched ---------
say "checking the host"
[ -d "$SRC/.git" ] || die "$SRC is not a git repository - see the first-time steps at the top of this file"
[ -f "$ENV_FILE" ] || die "no secrets at $ENV_FILE - compose cannot interpolate POSTGRES_PASSWORD or JWT_SECRET without it"
[ -f "$COMPOSE_DIR/docker-compose.yml" ] || die "no compose file at $COMPOSE_DIR"
command -v docker > /dev/null || die "docker is not on PATH"
mkdir -p "$BACKUPS"

compose() { docker compose --env-file "$ENV_FILE" -f "$COMPOSE_DIR/docker-compose.yml" "$@"; }

# A hand-edit on the server is how a host quietly stops matching the
# repository. It is about to be overwritten either way; the point of saying
# so is that it ends up in the log rather than nowhere.
DIRTY=$(git_ status --porcelain --untracked-files=no)
if [ -n "$DIRTY" ]; then
    printf 'these tracked files differ from the last deploy and are about to be overwritten:\n%s\n' "$DIRTY"
fi

# --- the database, before anything can migrate it --------------------
say "backing up the database"
POSTGRES_CID=$(compose ps -q postgres || true)
if [ -n "$POSTGRES_CID" ]; then
    STAMP=$(date -u +%Y%m%dT%H%M%SZ)
    BACKUP="$BACKUPS/vibessh-$STAMP.sql.gz"
    docker exec "$POSTGRES_CID" sh -c 'pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB"' | gzip > "$BACKUP"
    chmod 600 "$BACKUP"
    # A backup you find out about while restoring is worse than no backup.
    # `pg_dump`'s own status is lost here - in POSIX sh a pipeline reports
    # gzip's, and gzip is perfectly happy to compress a truncated dump - so
    # the file is what gets checked. A complete dump contains the marker line
    # pg_dump writes when it finishes.
    #
    # Checked over the last several lines rather than the last three: pg_dump
    # from PostgreSQL 17 appends a `\unrestrict <token>` line (and a blank)
    # *after* the "dump complete" marker, which pushed the marker out of a
    # `tail -3` window and made every good backup look truncated. The marker
    # is still the last thing that proves completion - a genuinely truncated
    # dump does not reach it - so widening the window keeps the guard honest
    # while tolerating whatever psql meta-commands pg_dump now trails it with.
    # A bad dump is moved out of the way rather than left under a name that
    # reads like a good one. Whoever restores in a hurry picks the newest
    # `vibessh-*.sql.gz` and will not be reading this script at the time.
    if [ ! -s "$BACKUP" ]; then
        mv "$BACKUP" "$BACKUP.unusable"
        die "the backup was empty, kept as $BACKUP.unusable - stopping before anything migrates"
    fi
    if ! gzip -dc "$BACKUP" | tail -10 | grep -q 'PostgreSQL database dump complete'; then
        mv "$BACKUP" "$BACKUP.unusable"
        die "the backup was truncated, kept as $BACKUP.unusable - stopping before anything migrates"
    fi
    printf 'wrote %s (%s bytes)\n' "$BACKUP" "$(wc -c < "$BACKUP")"

    # Oldest first, keep the newest `KEEP_BACKUPS`. These are kilobytes each,
    # so this is tidiness rather than disk pressure.
    ls -1t "$BACKUPS"/vibessh-*.sql.gz 2>/dev/null | tail -n "+$((KEEP_BACKUPS + 1))" | while read -r old; do
        rm -f "$old"
        printf 'removed old backup %s\n' "$old"
    done
else
    printf 'no postgres container is running, so there is nothing to back up yet\n'
fi

# --- the source ------------------------------------------------------
say "fetching $REF"
git_ fetch --quiet --prune origin '+refs/heads/*:refs/remotes/origin/*' '+refs/tags/*:refs/tags/*'
BEFORE=$(git_ rev-parse HEAD)
git_ rev-parse --verify --quiet "$REF^{commit}" > /dev/null || die "no such ref: $REF"
AFTER=$(git_ rev-parse "$REF^{commit}")

if [ "$BEFORE" = "$AFTER" ]; then
    printf 'already at %s - rebuilding anyway, in case the last deploy did not finish\n' "$(echo "$AFTER" | cut -c1-8)"
else
    printf 'what is about to be deployed:\n'
    git_ log --oneline --no-decorate "$BEFORE..$AFTER" || true
fi
git_ reset --hard --quiet "$AFTER"

# --- the containers --------------------------------------------------
say "building and starting"
compose up -d --build

# --- proof it came back ----------------------------------------------
#
# `up -d` returns when the container has started, which is not the same as
# the process inside it having connected to the database and run its
# migrations. Without this a failed deploy looks exactly like a good one
# until somebody opens the app.
say "waiting for $HEALTH_URL"
WAITED=0
while [ "$WAITED" -lt "$HEALTH_TIMEOUT" ]; do
    if curl -fsS --max-time 5 "$HEALTH_URL" > /tmp/vibessh-health.$$ 2>/dev/null; then
        printf '%s\n' "$(cat /tmp/vibessh-health.$$)"
        rm -f /tmp/vibessh-health.$$
        HEALTHY=1
        break
    fi
    sleep 3
    WAITED=$((WAITED + 3))
done
rm -f /tmp/vibessh-health.$$

SHORT=$(echo "$AFTER" | cut -c1-8)
if [ "${HEALTHY:-0}" != "1" ]; then
    printf '\nthe backend did not answer within %ss. Its last lines:\n\n' "$HEALTH_TIMEOUT"
    compose logs --tail 50 backend || true
    # Deliberately not rolled back. Migrations only run forwards, so putting
    # the old image back on a database the new one has already migrated is a
    # second failure on top of the first. The backup above is the way back,
    # and it needs a person who knows what broke.
    printf '\nnothing has been rolled back: migrations run forwards only.\n'
    # An `a && b` here instead of an `if` would end the script under `set -e`
    # whenever there was no backup, and the log line below would never be
    # written - losing the record of exactly the deploy worth recording.
    if [ -n "${BACKUP:-}" ]; then
        printf 'the database as it was before this deploy: %s\n' "$BACKUP"
    fi
    printf '%s  %s  FAILED (health)\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$SHORT" >> "$LOG"
    exit 1
fi

printf '%s  %s  ok\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$SHORT" >> "$LOG"
say "deployed $SHORT"
