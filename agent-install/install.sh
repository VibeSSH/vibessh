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

REPO="${VIBESSH_INSTALL_REPO:-VibeSSH/vibessh}"
VERSION="${VIBESSH_INSTALL_VERSION:-latest}"
BASE_URL="${VIBESSH_INSTALL_BASE_URL:-https://github.com/${REPO}/releases}"
BIN_DIR="${VIBESSH_INSTALL_BIN_DIR:-/usr/local/bin}"
CONFIG_DIR="${VIBESSH_INSTALL_CONFIG_DIR:-/etc/vibessh/agent}"
DATA_DIR="${VIBESSH_INSTALL_DATA_DIR:-/var/lib/vibessh/agent}"
SERVICE_USER="${VIBESSH_INSTALL_USER:-vibessh-agent}"
UNIT_PATH="${VIBESSH_INSTALL_UNIT_PATH:-/etc/systemd/system/vibessh-agent.service}"
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

# No code-signing pipeline exists yet (would need a real release CI and a
# signing key) - checksum verification proves the download wasn't corrupted
# or MITM'd in a way that doesn't also compromise the checksum file itself.
# Real signature verification is a reasonable Etap K follow-up, not skipped
# by accident.
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

    log "verifying checksum"
    verify_checksum "$tmp_dir/$asset" "$tmp_dir/$asset.sha256"

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
    log "creating ${CONFIG_DIR} and ${DATA_DIR}"
    install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$CONFIG_DIR"
    install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$DATA_DIR"
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

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
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
    log "starting vibessh-agent service"
    systemctl daemon-reload
    systemctl enable --now vibessh-agent.service
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
