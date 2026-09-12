# Assembles latest.json from whatever the platform builds produced.
#
# Kept out of the workflow file because it has to reason about two platforms
# at once - which installer of the several a bundler emits is the one the
# updater can actually take, and whether it is signed - and that is a poor fit
# for a shell step embedded in YAML.
#
# The URL cannot be taken from the build: assets live in a different, public
# repository, so it is derived here from the version instead.
import json
import os
import pathlib
import sys
import datetime

RELEASES_REPO = "VibeSSH/vibessh-releases"

# Which artifact each platform updates from, in preference order. Only one of
# these can be an update: a .deb is a package manager's business, and the
# updater has no way to install one.
WANTED = {
    "windows-x86_64": ("-setup.exe",),
    "linux-x86_64": (".AppImage",),
}

root = pathlib.Path("dist-release")
version = json.loads(pathlib.Path("apps/desktop/src-tauri/tauri.conf.json").read_text(encoding="utf8"))["version"]
tag = "v" + version

platforms = {}
for platform, suffixes in WANTED.items():
    for suffix in suffixes:
        installer = next((p for p in sorted(root.iterdir()) if p.name.endswith(suffix)), None)
        if installer is None:
            continue

        signature = root / (installer.name + ".sig")
        if not signature.exists():
            # Never published unsigned. Without the signature the app has no
            # way to tell a real update from anything else that manages to
            # answer the update URL.
            print("::error::%s has no signature beside it" % installer.name)
            sys.exit(1)

        platforms[platform] = {
            "signature": signature.read_text(encoding="utf8").strip(),
            "url": "https://github.com/%s/releases/download/%s/%s" % (RELEASES_REPO, tag, installer.name),
        }
        break

if not platforms:
    print("::error::no installer for any platform - nothing to publish")
    sys.exit(1)

# A missing platform is worth saying out loud rather than quietly shipping a
# manifest that leaves those users with nothing to update to.
for platform in WANTED:
    if platform not in platforms:
        print("::warning::no installer for %s, so it will not be offered an update" % platform)

manifest = {
    "version": version,
    "notes": "VibeSSH " + tag,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": platforms,
}

out = root / "latest.json"
out.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf8")
print("--- latest.json ---")
print(out.read_text(encoding="utf8"))
