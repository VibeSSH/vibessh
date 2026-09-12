# Repository structure

All five steps are done. What follows is the reasoning, and what each one cost. What is here today, why it reads as a
mess, what it should look like, and in which order to get there without
breaking the build.

## What is actually mixed

The repository holds five separate units of software. Four of them live in a
named directory. The fifth — the frontend — *is* the repository root:

| At the root | Belongs to |
| --- | --- |
| `src/` (316 files), `index.html`, `public/`, `dist/` | the desktop UI |
| `vite.config.ts`, `vitest.config.ts`, `tsconfig*.json` | the desktop UI |
| `package.json`, `bun.lock`, `node_modules/` | the desktop UI |
| `Cargo.toml`, `Cargo.lock`, `clippy.toml`, `target/` | the Rust workspace |
| `apps/desktop/src-tauri/`, `agent/`, `protocol/`, `backend/` | its four members |
| `installer/` | `src-tauri` only (NSIS bitmaps) |
| `apps/agent/install/` | `agent` only |
| `scripts/` | one repo-wide script, one frontend script |
| `docs/` | prose, plus `docs/guide/` which is a **build input** |
| `AUDIT_REPORT.md`, `FIX_PLAN.md` | planning, not product |

So the five problems, named precisely:

1. **`src/` reads as "the source of the repository"** when it is the source of
   one module out of five. This is the single biggest source of confusion and
   the cheapest to fix.
2. **Two package managers at the same level.** `Cargo.toml` + `target/` beside
   `package.json` + `node_modules/` + `dist/`, with nothing in the tree saying
   which of the twenty root entries belongs to which toolchain.
3. **`src-tauri` is named after its framework, not its contents.** It holds
   202 files of domain logic — blueprints, services, storage, firewall,
   Pterodactyl import, the AI assistant — behind a name that promises Tauri
   glue.
4. **Loose directories with one owner each.** `installer/` is read only by
   `apps/desktop/src-tauri/tauri.conf.json`; `apps/agent/install/` ships only with the agent.
   Sitting at the root, they look repo-wide.
5. **`backend/` is a deployable service** with its own Dockerfile and
   compose file, sitting as a peer of `protocol/`, a nine-file DTO crate.
   The tree does not distinguish "this deploys" from "this is a library".

## The organising rule

One directory per unit that ships to somebody, one for what several of them
share, one for what is not code. Directories named after what they contain,
never after the framework that happens to build them.

## Target layout

```
vibessh/
├── apps/                      things that ship to somebody
│   ├── desktop/
│   │   ├── ui/                was: src/, index.html, public/, vite+vitest+tsconfig, package.json
│   │   ├── src-tauri/         was: src-tauri/ - name kept, see below
│   │   └── installer/         was: installer/ - NSIS bitmaps, read only from here
│   ├── agent/                 was: agent/
│   │   └── install/           was: apps/agent/install/
│   └── backend/               was: backend/ - unchanged, already self-contained
├── crates/
│   └── protocol/              was: protocol/ - the wire format both ends share
├── shared/
│   └── guide/                 was: docs/guide/ - compiled into ui AND src-tauri
├── docs/                      prose, except the four files the AI corpus embeds
│   ├── architecture/          APPLICATIONS_ARCHITECTURE.md, future-host-mesh.md, navio.md
│   ├── security/              threat-model.md, security-review.md, agent-privileges.md
│   └── planning/              AUDIT_REPORT.md, FIX_PLAN.md, UI_AUDIT.md
├── tools/                     was: scripts/
├── Cargo.toml  Cargo.lock  clippy.toml
├── README.md  CHANGELOG.md  LICENSE.txt  AGENTS.md
└── .github/  .claude/
```

## Why each of those, specifically

**`apps/desktop/ui` and `apps/desktop/core`, not two entries under `apps/`.**
They are one product built into one binary; the UI is not deployable on its
own. Nesting says that. It also gives the frontend somewhere to put its own
`package.json`, `node_modules/` and `dist/` so that none of them are root
entries any more.

**`src-tauri` keeps its name, under `apps/desktop/`.** The argument for
renaming it to `core` still stands - Tauri is how it reaches a window, and
what is in there is the product. It lost to a concrete risk: the release
pipeline's `tauri-action` finds the project by looking for a `src-tauri` child
of `projectPath`, and that behaviour cannot be checked without cutting a real
tag. Renaming would have traded a verifiable arrangement for one whose first
failure is a broken release. Sitting beside `ui/` and `installer/` under
`apps/desktop/`, the parent already says what it is.

**`shared/guide`, not `docs/guide`.** This is the one piece of the current
layout that is actively misleading, and the code says so itself — "One corpus,
two readers". It is compiled into the Rust binary by `include_str!` and into
the frontend bundle by `import.meta.glob`. It is a build input that happens to
be written in Markdown, and filing it under `docs/` invites somebody to
reorganise prose and break two builds. Under `shared/` its role is visible.

**`docs/` for prose - almost.** This was stated too strongly. Three of these
documents are *also* compiled into the Rust binary: the AI assistant's corpus
embeds `APPLICATIONS_ARCHITECTURE.md`, `threat-model.md`, `agent-privileges.md`
and the `README.md` alongside the guide. Step 5 found that by failing to
compile. The rest is read by people, and the three sub-folders separate what a contributor needs
(architecture), what a reviewer needs (security), and what is a snapshot of
work in progress (planning). `AUDIT_REPORT.md` and `FIX_PLAN.md` are the two
that most obviously do not belong at the root beside `LICENSE.txt`.

**`tools/`, not `scripts/`.** Same contents, but the current folder mixes
`setup.ps1` (repo-wide) with `generate-lucide-subset.mjs` (generates an asset
for the UI). The generator should move to `apps/desktop/ui/tools/` and leave
`tools/` for genuinely repo-wide things.

**`crates/protocol` with one member.** Honest caveat: a bucket holding one
crate is not obviously better than the crate at the root. It earns its place
the moment a second shared library appears, and it makes the distinction the
tree currently lacks — `protocol` is a library, `agent` and `backend` are
programs. If that second crate never arrives, this is the part of the proposal
to drop.

## What constrains the move

These are measured from the code, not assumed. Every one of them breaks if a
directory moves and nothing else changes.

| Coupling | Where | Breaks if |
| --- | --- | --- |
| `include_str!("../../../shared/guide/*.md")` | `apps/desktop/src-tauri/src/ai/knowledge.rs`, 66 lines | the guide moves, or `src-tauri` does |
| `CARGO_MANIFEST_DIR` + `"../../../shared/guide"`, read at runtime | `knowledge.rs`'s corpus test | either moves |
| `include_str!` of `README.md`, `APPLICATIONS_ARCHITECTURE.md`, `threat-model.md`, `agent-privileges.md` | `knowledge.rs`, the AI corpus | the docs move, or the crate does |
| `path = "../protocol"` | `src-tauri/Cargo.toml`, `agent/Cargo.toml` | either crate moves |
| `projectPath` (absent, so the repository root) | `release.yml`'s `tauri-action` | `src-tauri` stops being a child of it |
| `import.meta.glob("../../docs/guide/*.md")` | `src/guide/guideDocs.ts`, `guideImages.ts` | the guide moves, or `src/` does |
| `frontendDist: "../dist"` | `apps/desktop/src-tauri/tauri.conf.json` | the frontend output moves |
| `beforeDevCommand: "bun run dev"` | `apps/desktop/src-tauri/tauri.conf.json` | `package.json` stops being at the root |
| `headerImage: "../installer/*.bmp"` | `apps/desktop/src-tauri/tauri.conf.json` | `installer/` moves |
| `alias "@" -> ./src` | `vite.config.ts` | the UI source moves |
| `import promptSource from "../../apps/desktop/src-tauri/src/ai/prompt.rs?raw"` | `src/config/navigation.promptParity.test.ts` | either side moves |
| `members = ["src-tauri", "agent", "protocol", "backend"]` | `Cargo.toml` | any crate moves |
| `workspaces: src-tauri`, `require('./apps/desktop/src-tauri/tauri.conf.json')`, `cp apps/desktop/src-tauri/icons/...` | `.github/workflows/release.yml` | `src-tauri` moves |
| `./scripts/setup.ps1` in the README and in contributor habit | `scripts/` | `scripts/` moves |

The guide corpus is the sharp one: it is the only asset with two compile-time
readers, and the two of them reach it by relative paths from opposite
directions.

The parity test is the one this table missed on its first pass, and it is
worth saying why: it is a *frontend test that imports a Rust source file*, to
check that the navigation the UI renders matches the one the AI prompt
describes. Nothing about its file name or location suggests it crosses a
module boundary. Doing step 1 found it in about a minute, because the test
suite failed; a survey by reading alone did not.

## Order of work

Staged so that each step is independently verifiable and the risky ones come
last. Every step ends with `cargo clippy -D warnings`, `cargo test`,
`bun run typecheck`, `bun run test`, `bun run build`, and a real
`bun run tauri build` for the ones that touch bundling.

**Step 1 — give the frontend a folder. Done.** `src/`, `index.html`,
`public/`, `package.json`, `vite.config.ts`, `vitest.config.ts` and
`tsconfig*.json` are in `apps/desktop/ui/`.

Two things went differently from the plan above, both worth recording:

*The root keeps a `package.json`.* The plan said to move it outright. The
Tauri CLI looks for `apps/desktop/src-tauri/` beside the directory it is invoked in and
does not search upwards, so moving the only manifest would have meant either
running `tauri` from a directory with no `src-tauri` beside it, or passing
`--config` and fighting every path it resolves relative to that. Instead the
root holds a Bun **workspace** manifest: the Tauri CLI, and six scripts that
delegate to the UI package. The root manifest is 22 lines against the UI's
sixty-odd dependencies, and - the part that paid for itself - `bun install`,
`bun run typecheck`, `bun run test` and `bun run build` still mean the same
thing from the repository root, so **CI and the release workflow needed no
changes at all**.

*The delegation is `bun run --filter`, not `bun --cwd`.* This Bun does not
accept `--cwd` before `run`. Verified that a failing script still exits
non-zero through `--filter`, since a CI gate that cannot fail is worse than no
gate.

`bun.lock` also stays at the root, which is where a workspace lockfile
belongs.

**Step 2 — move the loose owned folders. Done.** `installer/` is
`apps/desktop/installer/`, read only by `tauri.conf.json`'s nsis block; both
bitmap paths were checked by resolving them from the config's own directory.

The agent installer went to `apps/agent/install/` rather than `apps/agent/install/`:
the agent crate is still at the root until step 5, and creating `apps/agent/`
now would have meant two directories called agent, in two places, for the sake
of one intermediate commit. Inside the crate it already sits in its final
relative position, so step 5 carries it along for free.

**Step 3 — sort `docs/`. Done.** `architecture/`, `security/` and
`planning/`, with `AUDIT_REPORT.md` and `FIX_PLAN.md` coming in off the
repository root. `docs/guide/` is untouched, and `repository-structure.md`
stays at the top of `docs/` as the map of the rest.

A fourth folder has since joined them: `operations/`, for how the things we
actually run are deployed, starting with `hosted-backend.md`. It earns its
own heading rather than sitting under `architecture/` because it describes
one particular machine's state, which goes stale in a way a design document
does not.

The move itself was free; carrying its references was not, and this is the
part the plan underestimated by calling it "the cheapest step". These
documents are cited **193 times** across the codebase, nearly all from Rust
and TypeScript doc comments - `docs/APPLICATIONS_ARCHITECTURE.md` alone
appears 73 times. Every path-shaped reference was rewritten. Bare mentions in
prose ("three findings in `AUDIT_REPORT.md`") were left alone: they name a
document rather than point at a file, and rewriting those would have been
noise in a hundred comments.

**Step 4 — move the guide to `shared/guide/`. Done.** 66 `include_str!`
paths, three globs, and one that the table had not listed: a test in
`knowledge.rs` that reads the directory at *runtime* through
`CARGO_MANIFEST_DIR` to catch a guide page nobody registered. A comment
reference would have been harmless; that one would have failed the build.

Both readers were checked rather than assumed. The frontend bundle in
`dist/assets/*.js` contains the guide text, and
`guide_corpus_tests::every_guide_document_is_in_the_corpus` passes - which is
exactly the test that compares what is on disk against what was compiled in,
so it could not pass if the directory path were wrong.

**Step 5 — move the Rust crates. Done.** `src-tauri` to
`apps/desktop/src-tauri` (name kept, see above), `agent` to `apps/agent`,
`backend` to `apps/backend`, `protocol` to `crates/protocol`.

96 root-shaped path references, the workspace member list, two
`path = "../protocol"` dependencies, the rust-cache key, `projectPath` on
`tauri-action`, four paths in the release workflow, three `.gitignore` rules
and one in `.gitattributes`. Both halves of `tauri.conf.json` got *shorter*
rather than longer: the UI and the installer art are siblings now, so
`../apps/desktop/ui/dist` became `../ui/dist`.

The root script is `cd apps/desktop && tauri`, because the CLI looks for a
`src-tauri` child of the directory it runs in.

What this step found, by failing to compile: `knowledge.rs` embeds four
documents that are not the guide, so `docs/` is not prose only after all.

## What not to do

**Do not collapse the per-application Docker networks** to save address space,
and do not flatten `protocol` into `src-tauri` to save a crate. Both would
make the tree simpler and the product worse — the first is the isolation the
Vibe Network is premised on, the second is what keeps the agent from
depending on the desktop.

**Do not move `Cargo.lock`, `Cargo.toml` or `target/`.** A workspace root is
a real thing and belongs at the repository root; the crates move under it.

**Do not do steps 1 and 5 in one commit.** They break in different ways and
the failures are hard to tell apart once they are mixed.
