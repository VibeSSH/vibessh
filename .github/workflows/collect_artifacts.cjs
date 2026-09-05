// Copies what the build produced, plus each signature, into `dist`.
//
// The .cjs extension is load-bearing: this project's package.json sets
// "type": "module", so a plain .js file here would be an ES module and
// `require` would not exist in it.
//
// A file rather than an inline step because the same copying has to run on
// both a Windows and a Linux runner, and the two shells that come with them
// do not agree on quoting - which is how an earlier version of this ended up
// silently copying nothing on one of them.
//
// The paths come from `tauri-action`'s own `artifactPaths` output. Guessing
// them from the repository layout was the first attempt and it was wrong:
// this is a Cargo workspace, so the target directory is at the root rather
// than under `src-tauri`.
const fs = require("fs");
const path = require("path");

const paths = JSON.parse(process.env.ARTIFACT_PATHS || "[]");
if (paths.length === 0) {
  console.error("::error::the build reported no artifacts");
  process.exit(1);
}

fs.mkdirSync("dist", { recursive: true });

for (const artifact of paths) {
  // The signature sits beside the installer rather than being listed as an
  // artifact of its own, so it is looked for by name.
  for (const file of [artifact, artifact + ".sig"]) {
    if (fs.existsSync(file)) {
      fs.copyFileSync(file, path.join("dist", path.basename(file)));
      console.log("collected " + path.basename(file));
    }
  }
}
