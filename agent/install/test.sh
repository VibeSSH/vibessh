#!/bin/sh
# Exercises the parts of install.sh that don't need root or a real Linux
# box: architecture detection, the OS guard, and checksum verification.
# Everything else (systemd, useradd, the actual download) needs a real
# Linux VM/container to test - this is deliberately not a substitute for
# that, just what's honestly verifiable in a dev environment that doesn't
# have one.
#
# Failure paths call the real install.sh's die(), which calls `exit` - each
# of those is run in a `( subshell )` below so that only the subshell exits,
# not this whole test script.
set -eu

cd "$(dirname "$0")"

pass=0
fail=0

ok() {
    printf 'ok   - %s\n' "$1"
    pass=$((pass + 1))
}

fail_() {
    printf 'FAIL - %s\n' "$1"
    fail=$((fail + 1))
}

VIBESSH_INSTALL_SOURCE_ONLY=1
export VIBESSH_INSTALL_SOURCE_ONLY
. ./install.sh

# --- detect_arch: this machine's real uname -m, whatever it is ---
real_machine="$(uname -m)"
case "$real_machine" in
    x86_64|amd64) expected_target="linux-amd64" ;;
    aarch64|arm64) expected_target="linux-arm64" ;;
    *) expected_target="" ;;
esac
if [ -n "$expected_target" ]; then
    got="$(detect_arch)"
    if [ "$got" = "$expected_target" ]; then
        ok "detect_arch resolves '$real_machine' to '$expected_target'"
    else
        fail_ "detect_arch: expected '$expected_target' for '$real_machine', got '$got'"
    fi
else
    printf 'skip - detect_arch (unrecognized test-host arch %s)\n' "$real_machine"
fi

# --- detect_arch rejects an unknown machine type - runs the real function
# with uname faked in a subshell, not a reimplementation of its logic ---
if (
    uname() { echo "totally-unknown-arch"; }
    detect_arch
) >/dev/null 2>&1; then
    fail_ "detect_arch unexpectedly accepted an unknown architecture"
else
    ok "detect_arch rejects an unknown architecture"
fi

# --- detect_os: this box is never Linux under Git Bash/MSYS on Windows, so
# this genuinely exercises the rejection path, not a simulation of it ---
if (detect_os) >/dev/null 2>&1; then
    real_os="$(uname -s)"
    case "$real_os" in
        Linux) ok "detect_os accepts Linux" ;;
        *) fail_ "detect_os accepted non-Linux OS '$real_os'" ;;
    esac
else
    ok "detect_os rejects non-Linux (this host: $(uname -s))"
fi

# --- verify_checksum: real sha256 round trip using this system's own tools ---
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

printf 'vibessh-agent-test-binary-contents' > "$tmp_dir/binary"
printf '%s  binary\n' "$(sha256_of "$tmp_dir/binary")" > "$tmp_dir/binary.sha256"

if (verify_checksum "$tmp_dir/binary" "$tmp_dir/binary.sha256") >/dev/null 2>&1; then
    ok "verify_checksum accepts a matching checksum"
else
    fail_ "verify_checksum rejected a matching checksum"
fi

printf '0000000000000000000000000000000000000000000000000000000000000000  binary\n' > "$tmp_dir/binary.bad.sha256"
if (verify_checksum "$tmp_dir/binary" "$tmp_dir/binary.bad.sha256") >/dev/null 2>&1; then
    fail_ "verify_checksum accepted a mismatched checksum"
else
    ok "verify_checksum rejects a mismatched checksum"
fi

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
