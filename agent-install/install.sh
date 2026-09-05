#!/bin/sh
# VibeSSH Agent installer.
#
#   curl -fsSL <release-url>/install.sh | sudo sh
#
# Downloads, verifies, and installs the vibe-agent binary as a systemd
# service. That's all this script does - pairing, config parsing, and every
# other bit of real agent behavior lives in the binary itself (see the
# project rules: "Nie wkładaj właściwej logiki aplikacji do skryptu shell").
#
# After this finishes, pair the agent from a separate step:
#   vibe-agent pair <CODE-SHOWN-IN-VIBESSH>
#
# Overridable via environment variables (see README in this directory for
# the full list) - defaults assume a fresh Debian/Ubuntu/RHEL-family Linux
# VPS with systemd and GNU coreutils, which covers the large majority of
# what "Linux first" means for a VPS-hosted game/app server.
set -eu

# The public releases repository, not the source one. GitHub serves release
# assets from a private repository only with an Authorization header, so
# pointing this at the source meant every download - the script itself
# included - answered 404.
REPO="${VIBESSH_INSTALL_REPO:-VibeSSH/vibessh-releases}"
VERSION="${VIBESSH_INSTALL_VERSION:-latest}"
BASE_URL="${VIBESSH_INSTALL_BASE_URL:-https://github.com/${REPO}/releases}"
BIN_DIR="${VIBESSH_INSTALL_BIN_DIR:-/usr/local/bin}"
# Root-owned parent - holds managed-units.conf, which the service user must
# never be able to write (see ensure_directories/install_polkit_rule).
BASE_CONFIG_DIR="${VIBESSH_INSTALL_BASE_CONFIG_DIR:-/etc/vibessh}"
CONFIG_DIR="${VIBESSH_INSTALL_CONFIG_DIR:-${BASE_CONFIG_DIR}/agent}"
DATA_DIR="${VIBESSH_INSTALL_DATA_DIR:-/var/lib/vibessh/agent}"
SERVICE_USER="${VIBESSH_INSTALL_USER:-vibessh-agent}"
UNIT_PATH="${VIBESSH_INSTALL_UNIT_PATH:-/etc/systemd/system/vibessh-agent.service}"
POLKIT_RULE_PATH="${VIBESSH_INSTALL_POLKIT_RULE_PATH:-/etc/polkit-1/rules.d/49-vibessh-agent.rules}"
MANAGED_UNITS_FILE="${BASE_CONFIG_DIR}/managed-units.conf"
# Skips the download+checksum steps and installs a binary already present on
# this machine instead. Meant for local development/CI, not part of the
# public one-liner - lets the rest of this script (user/dirs/unit/service)
# be tested without a published release to fetch.
LOCAL_BINARY="${VIBESSH_INSTALL_LOCAL_BINARY:-}"

log() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

require_root() {
    if [ "$(id -u)" -ne 0 ]; then
        die "this installer needs root (try: curl ... | sudo sh)"
    fi
}

detect_os() {
    os="$(uname -s)"
    case "$os" in
        Linux) ;;
        *) die "unsupported OS '$os' - VibeSSH Agent currently supports Linux only" ;;
    esac
}

# Prints the release asset target name (e.g. "linux-amd64") on success.
detect_arch() {
    machine="$(uname -m)"
    case "$machine" in
        x86_64|amd64) echo "linux-amd64" ;;
        aarch64|arm64) echo "linux-arm64" ;;
        *) die "unsupported architecture '$machine' - only linux-amd64 and linux-arm64 are published" ;;
    esac
}

download() {
    url="$1"
    dest="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$url" -O "$dest"
    else
        die "neither curl nor wget is available"
    fi
}

sha256_of() {
    file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        die "neither sha256sum nor shasum is available to verify the download"
    fi
}

# checksum_file holds "<hex digest>  <filename>" (sha256sum's own format).
verify_checksum() {
    binary_file="$1"
    checksum_file="$2"
    expected="$(awk '{print $1}' "$checksum_file")"
    actual="$(sha256_of "$binary_file")"
    [ "$expected" = "$actual" ] || die "checksum mismatch (expected $expected, got $actual)"
}

# Verifies the release signature over a downloaded file.
#
# **Why this exists and the checksum does not replace it.** A checksum proves
# the download matches a file published beside it. It says nothing about who
# published it: anyone able to serve the binary can serve a matching checksum.
# This installs a root-owned systemd service, so "the download was not
# corrupted" is not the question worth answering.
#
# The signature is the same minisign key the desktop app's updater checks,
# and its public half is below - it is public by construction, since every
# copy of the app carries it.
#
# **Why openssl rather than minisign.** Almost no VPS has minisign installed
# and almost all have openssl. What that costs is this function: Tauri writes
# the prehashed minisign variant, so the Ed25519 signature covers the
# BLAKE2b-512 of the file rather than the file, and the raw key has to be
# wrapped in DER before openssl will read it.
PUBKEY_BASE64="${VIBESSH_INSTALL_PUBKEY:-RWT3fZFYjZIq2uShgMUHstxNCuGSvunuLV3bGKnn92dLSsiSoNslDm+i}"

verify_signature() {
    binary_file="$1"
    signature_file="$2"
    work_dir="$3"

    command -v openssl >/dev/null 2>&1 || die "openssl is needed to verify the download's signature - install it and run this again"

    # The last 32 bytes of the key are the Ed25519 key itself; the first ten
    # are the algorithm and the key id.
    printf '%s' "$PUBKEY_BASE64" | base64 -d 2>/dev/null | tail -c 32 > "$work_dir/pub.raw" \
        || die "the built-in public key could not be decoded"
    [ -s "$work_dir/pub.raw" ] || die "the built-in public key could not be decoded"

    # A fixed DER header for an Ed25519 SubjectPublicKeyInfo, written as octal
    # so this needs no xxd - which a minimal image often does not have.
    { printf '\060\052\060\005\006\003\053\145\160\003\041\000'; cat "$work_dir/pub.raw"; } > "$work_dir/pub.der"

    # Tauri's .sig is base64 of a whole minisign file; its second line holds
    # the signature, whose last 64 bytes are the Ed25519 signature.
    base64 -d < "$signature_file" 2>/dev/null | sed -n '2p' | base64 -d 2>/dev/null | tail -c 64 > "$work_dir/sig.raw" \
        || die "the signature file could not be decoded"
    [ -s "$work_dir/sig.raw" ] || die "the signature file could not be decoded"

    openssl dgst -blake2b512 -binary "$binary_file" > "$work_dir/hash.bin" \
        || die "this openssl cannot compute BLAKE2b-512, which the signature is over"

    openssl pkeyutl -verify -pubin -inkey "$work_dir/pub.der" -keyform DER \
        -rawin -in "$work_dir/hash.bin" -sigfile "$work_dir/sig.raw" >/dev/null 2>&1 \
        || die "the downloaded agent is not signed by VibeSSH - refusing to install it"
}

fetch_and_verify() {
    target="$1" # e.g. linux-amd64
    tmp_dir="$2"
    asset="vibe-agent-${target}"

    if [ "$VERSION" = "latest" ]; then
        asset_url="${BASE_URL}/latest/download/${asset}"
    else
        asset_url="${BASE_URL}/download/${VERSION}/${asset}"
    fi

    log "downloading ${asset_url}"
    download "$asset_url" "$tmp_dir/$asset"
    download "${asset_url}.sha256" "$tmp_dir/$asset.sha256"
    download "${asset_url}.sig" "$tmp_dir/$asset.sig"

    # The checksum stays as the cheap answer to "did this arrive intact",
    # and runs first so a truncated download is reported as that rather than
    # as a signature failure.
    log "verifying checksum"
    verify_checksum "$tmp_dir/$asset" "$tmp_dir/$asset.sha256"

    log "verifying signature"
    verify_signature "$tmp_dir/$asset" "$tmp_dir/$asset.sig" "$tmp_dir"

    echo "$tmp_dir/$asset"
}

install_binary() {
    src="$1"
    log "installing binary to ${BIN_DIR}/vibe-agent"
    install -m 0755 "$src" "${BIN_DIR}/vibe-agent"
}

ensure_service_user() {
    if id "$SERVICE_USER" >/dev/null 2>&1; then
        return
    fi
    log "creating system user '${SERVICE_USER}'"
    if command -v useradd >/dev/null 2>&1; then
        useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
    elif command -v adduser >/dev/null 2>&1; then
        adduser --system --no-create-home --shell /usr/sbin/nologin --disabled-password "$SERVICE_USER"
    else
        die "neither useradd nor adduser is available to create the service user"
    fi
}

ensure_directories() {
    log "creating ${BASE_CONFIG_DIR}, ${CONFIG_DIR}, and ${DATA_DIR}"
    # root-owned: the service user must not be able to rewrite its own
    # systemd-management allowlist by replacing the directory entry, which
    # a service-user-owned parent would allow regardless of the file's own
    # permissions.
    install -d -m 0755 -o root -g root "$BASE_CONFIG_DIR"
    install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$CONFIG_DIR"
    install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$DATA_DIR"

    if [ ! -f "$MANAGED_UNITS_FILE" ]; then
        log "creating empty ${MANAGED_UNITS_FILE} (nothing authorized until an admin adds units here)"
        cat > "$MANAGED_UNITS_FILE" <<'EOF'
# One systemd unit name per line, e.g.:
#   nginx.service
#   mariadb.service
# vibessh-agent may only start/stop/restart/enable/disable units listed
# here (see /etc/polkit-1/rules.d/49-vibessh-agent.rules) - empty means it
# can manage none. Root-owned on purpose: the agent can read this file but
# must never be able to write it.
EOF
        chown root:root "$MANAGED_UNITS_FILE"
        chmod 0644 "$MANAGED_UNITS_FILE"
    fi
}

# Etap G's privilege analysis (see docs/agent-privileges.md in the main repo)
# concluded systemd unit management is the one operation with a clean,
# narrow polkit-based solution available today: this rule lets the service
# user start/stop/restart/enable/disable ONLY units listed in
# managed-units.conf, nothing else - not arbitrary units, not Docker, not
# other users' processes. Those still need root and stay out of scope until
# a feature that actually needs them exists (see the doc for why).
install_polkit_rule() {
    if ! command -v polkitd >/dev/null 2>&1 && [ ! -d /etc/polkit-1 ]; then
        log "polkit not found - skipping systemd-unit authorization rule (Quick Actions targeting systemd will have nothing to authorize them yet)"
        return
    fi

    log "installing polkit rule to ${POLKIT_RULE_PATH}"
    mkdir -p "$(dirname "$POLKIT_RULE_PATH")"
    cat > "$POLKIT_RULE_PATH" <<EOF
// Managed by VibeSSH's install.sh - do not edit by hand, edit
// ${MANAGED_UNITS_FILE} instead (one unit name per line).
//
// Authorizes ${SERVICE_USER} to start/stop/restart/enable/disable ONLY the
// systemd units listed in that file. Everything else - other units, other
// actions (kill, set-property), other users - falls through to polkit's
// normal default (deny for non-root).
polkit.addRule(function(action, subject) {
    var unitActions = [
        "org.freedesktop.systemd1.manage-units",
        "org.freedesktop.systemd1.manage-unit-files"
    ];
    if (unitActions.indexOf(action.id) === -1) {
        return polkit.Result.NOT_HANDLED;
    }
    if (subject.user !== "${SERVICE_USER}") {
        return polkit.Result.NOT_HANDLED;
    }

    // manage-units also carries a "verb" detail; only the ordinary
    // lifecycle verbs are authorized - not "kill" (arbitrary signals) or
    // "set-property" (arbitrary resource-limit changes).
    var verb = action.lookup("verb");
    if (verb) {
        var allowedVerbs = ["start", "stop", "restart", "try-restart", "reload", "reload-or-restart", "reload-or-try-restart"];
        if (allowedVerbs.indexOf(verb) === -1) {
            return polkit.Result.NOT_HANDLED;
        }
    }

    var unit = action.lookup("unit");
    if (!unit) {
        return polkit.Result.NOT_HANDLED;
    }

    var allowedUnits;
    try {
        allowedUnits = polkit.spawn(["/bin/cat", "${MANAGED_UNITS_FILE}"]).split("\n");
    } catch (e) {
        return polkit.Result.NOT_HANDLED; // fail closed if the file is missing/unreadable
    }

    for (var i = 0; i < allowedUnits.length; i++) {
        var line = allowedUnits[i].replace(/^\s+|\s+$/g, "");
        if (line === "" || line.charAt(0) === "#") continue;
        if (line === unit) {
            return polkit.Result.YES;
        }
    }
    return polkit.Result.NOT_HANDLED;
});
EOF
    chown root:root "$POLKIT_RULE_PATH"
    chmod 0644 "$POLKIT_RULE_PATH"
    # polkitd watches its rules directories and reloads automatically, but
    # doing it explicitly here means the rule is guaranteed active by the
    # time this script exits instead of racing an inotify event.
    if command -v systemctl >/dev/null 2>&1 && systemctl is-active --quiet polkit.service 2>/dev/null; then
        systemctl reload-or-restart polkit.service
    fi
}

# A baseline hardened unit - not the full privilege analysis Etap G does
# (that also covers which agent features need more than this, e.g. Docker
# socket access, and what capability/polkit/sudo-helper model covers that
# gap) but a real agent has no business running as root by default, so this
# doesn't wait for that analysis to at least not do the obviously wrong thing.
write_unit() {
    log "writing systemd unit to ${UNIT_PATH}"
    cat > "$UNIT_PATH" <<EOF
[Unit]
Description=VibeSSH Agent
After=network-online.target
Wants=network-online.target
# Give up after 5 crashes within 60s instead of restarting forever - a
# persistently-crashing agent should surface as a failed unit for an admin
# to look at, not spin and spam the journal indefinitely.
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
SyslogIdentifier=vibessh-agent
ExecStart=${BIN_DIR}/vibe-agent
Environment=VIBESSH_AGENT_DATA_DIR=${DATA_DIR}
Environment=VIBESSH_AGENT_CONFIG_DIR=${CONFIG_DIR}
Restart=on-failure
RestartSec=2
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR}

[Install]
WantedBy=multi-user.target
EOF
}

start_service() {
    systemctl daemon-reload
    # `enable --now` on an already-running, already-enabled unit is a no-op
    # - re-running this script to deploy an updated binary would silently
    # keep the old one running otherwise. `restart` after `enable` covers
    # both the fresh-install and the upgrade case.
    if systemctl is-active --quiet vibessh-agent.service 2>/dev/null; then
        log "restarting vibessh-agent service to pick up the new binary"
        systemctl enable vibessh-agent.service
        systemctl restart vibessh-agent.service
    else
        log "starting vibessh-agent service"
        systemctl enable --now vibessh-agent.service
    fi
}

print_status() {
    echo
    log "VibeSSH Agent installed and running."
    systemctl --no-pager --lines=0 status vibessh-agent.service || true
    echo
    echo "Next step: pair this agent with your desktop app."
    echo "  vibe-agent pair <CODE-SHOWN-IN-VIBESSH>"
}

main() {
    require_root
    detect_os
    target="$(detect_arch)"

    if [ -n "$LOCAL_BINARY" ]; then
        log "using local binary ${LOCAL_BINARY} (skipping download/checksum)"
        [ -f "$LOCAL_BINARY" ] || die "VIBESSH_INSTALL_LOCAL_BINARY points to a file that doesn't exist"
        binary_path="$LOCAL_BINARY"
    else
        tmp_dir="$(mktemp -d)"
        trap 'rm -rf "$tmp_dir"' EXIT
        binary_path="$(fetch_and_verify "$target" "$tmp_dir")"
    fi

    install_binary "$binary_path"
    ensure_service_user
    ensure_directories
    install_polkit_rule
    write_unit
    start_service
    print_status
}

# Sourcing this file with VIBESSH_INSTALL_SOURCE_ONLY=1 set loads every
# function above without running any of them - what the test suite in this
# directory uses to exercise detect_arch/detect_os/verify_checksum for real
# without needing root or a Linux box to run the full install on.
if [ -z "${VIBESSH_INSTALL_SOURCE_ONLY:-}" ]; then
    main "$@"
fi
