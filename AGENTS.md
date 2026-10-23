# AGENTS.md — handoff for AI assistants (and humans)

**Rule from the project owner (Giovanni): every session must leave this file up to date, so
the next AI can continue exactly where the previous one stopped.** Update the "Current
state" and "Next steps" sections and add a line to the session log before you finish.

The owner is a beginner with Git/Rust and prefers Italian in conversation. Explain every
action they must take in plain language. Code, comments and docs stay in English.

## Project in one paragraph
Calibro is an open-source, Shapr3D-like precision CAD app in Rust. Workspace crates:
`calibro-sketch` (sketch model, constraint solver, regions), `calibro-kernel` (adapter over
the truck B-rep kernel), `calibro-core` (feature history, recompute engine, undo/redo),
`calibro-project` (native `.calibro` ZIP format), `calibro-io` (STEP/STL/OBJ/GLB), and `app/`
(eframe/egui UI + wgpu renderer, desktop and WebAssembly). Read `docs/ARCHITECTURE.md` first,
then `docs/FEATURE_TRACKER.md` (full scope + status), `docs/TEST_REPORT.md` (evidence).

## Ground rules
- Toolchain is pinned in `rust-toolchain.toml` (Rust **1.97.0**). Workflows use the same
  version (`dtolnay/rust-toolchain@master` with `toolchain: 1.97.0`). Upgrade all together.
- Before committing: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (GPU tests need a Vulkan driver; on headless Linux install
  `mesa-vulkan-drivers` and set `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`).
- Never mark a feature "Tested" in the tracker without an automated test or a documented
  manual check. Never claim something works because the code compiles.
- Kernel calls go through `calibro_kernel::guarded` (panics → errors on native). truck
  booleans fail on spheres and exactly coplanar faces — see tracker §8 and
  `cargo run --release -p calibro-kernel --example boolean_probe`.
- The `.calibro` format is versioned (`calibro-project`, `FORMAT_VERSION`). Any change to
  `Document` serialization needs a version bump + `migrate()` + a round-trip test.
- Don't push to `main`, create tags/releases, or change repo settings without the owner's
  explicit OK in the conversation.

## Repository & deployment
- GitHub: https://github.com/giovaceciliato/calibro (default branch `main`; a `beta` branch exists, created by the owner via web upload)
- Web app (GitHub Pages, built by `.github/workflows/pages.yml`): https://giovaceciliato.github.io/calibro/
- `ci.yml`: fmt + clippy + tests + wasm build. `release.yml`: Windows/macOS/Linux binaries,
  attached to a GitHub Release when a tag `v*` is pushed. Release text: `RELEASE_NOTES.md`.
- Useful local tools used so far: `crates/calibro-io/examples/{export_samples,showcase}.rs`.

## Current state (update every session)
_Last updated: 2026-10-23 16:30 (status audit after PRs #1–#4)._

**Quick summary (read this first; the history is in the session log and docs/TEST_REPORT.md):**
- `main` = `d92df1b`. Locally re-checked on Rust stable + lavapipe: clippy `-D warnings` clean,
  **72 tests pass** (1 diagnostic ignored). CI and Pages green; web app live.
- Features on `main`: sketcher + solver (circles now also by drag / exact radius, PR #4),
  extrude/revolve/primitives/booleans/patterns/mirror, Fillet/Chamfer/Shell/Sweep/Loft (first
  versions, limits in tracker), Space = sketch on selected face, auto-cut when extruding into
  a body, dark-blue UI (PR #1), native `.calibro` format v2, STEP/STL/OBJ/GLB export.
- **Open PR #5** `feat/edge-drag-handles` (on-model drag handles for fillet/chamfer, 85 tests,
  CI green, mergeable). Waiting for the owner to review/merge.
- **Releases** (tags have NO `v` prefix; `release.yml` was changed to accept that):
  - `0.1.1` (prerelease): assets present — Windows `.exe` + `.zip`, macOS arm64 `.tar.gz`,
    Linux `.tar.gz`, Fedora RPM, portable Linux package, `SHA256SUMS.txt` (attached by the
    *Recover release downloads* workflow). Built from `857cf0c`, i.e. **without** PR #3/#4
    (circle drawing fix) and without PR #5.
  - `0.1.0` and `beta`: releases with **no assets** (tags created before the workflow handled
    them). Candidates for deletion by the owner to avoid confusing visitors.
  - Package version mismatch: `Cargo.toml` is still `0.1.0`, so the 0.1.1 release contains
    `calibro-0.1.0-1.x86_64.rpm`. Bump `workspace.package.version` before the next release.
- AI sessions cannot push tags or call the releases API (HTTP 403). The owner creates
  releases on github.com; the workflow builds and attaches all packages.
- Never launched on real hardware: Windows/macOS binaries, RPM on real Fedora.

## Next steps (in order)
1. Owner: review/merge PR #5 (edge drag handles).
2. Bump version to 0.1.2 in `Cargo.toml` (+ RELEASE_NOTES.md), then owner publishes release
   `0.1.2` from `main` so desktop users get the circle fix, drag handles and modify tools.
3. Owner: delete the empty `0.1.0` and `beta` releases (optional cleanup).
4. Get feedback from real Windows/macOS/Fedora runs.
5. Modify tools: corner fillets, curved-edge fillet/chamfer, shell of cylinders, multi-section
   loft, click-through UI test of Sweep/Loft creation.
6. Stage 2 (tracker §9): on-canvas dimension editing, trim/extend, topological naming v1
   (sketch-on-face and fillet edges are stored geometrically and break when earlier edits move
   them), 3MF export, hide cylinder seam edges, capped section view.

## Session log
- 2026-10-22 — Stage 1 built: research, architecture, all crates, app, native format, exports,
  CI/Pages/release workflows, docs. 58 tests passing. Desktop UI verified on Xvfb.
- 2026-10-23 — Owner uploaded to GitHub. Pages deploy and desktop builds succeeded; CI failed
  at Clippy (Rust 1.99). Pinned toolchain, made layout responsive, fixed title spam, wrote
  release notes, added this handoff file.
- 2026-10-23 (later) — Owner connected GitHub + installed the Claude app; pushed d11b79d; CI and Pages green; verified the live web app.
- 2026-10-23 (later) — Built Fedora RPM + portable Linux package on request; added packaging to the release workflow (pushed to main; no release yet).
- 2026-10-23 (later) — Shapr3D-style auto cut when extruding into a body; face-sketch axis orientation fix; rebuilt Fedora packages.
- 2026-10-23 (later) — Fillet/Chamfer/Shell/Sweep/Loft + Space to sketch on a face (owner request); format v2; pushed to main.
- 2026-10-23 (later) — Release requested; tag push/release API refused for the AI session (403). release.yml now sets name + body from RELEASE_NOTES.md; owner to publish v0.1.0 from the web UI.

- 2026-10-23 — Dark-blue UI refresh inspired by Shapr3D, implemented on a separate review branch. Shared egui/viewport palette, vector tool icons, shortcuts, project header, panel hierarchy and start card; see test report for validation.
- 2026-10-23 — GitHub publication blocked by automatic approval review (explicit source-upload authorization requested). Changes, screenshots and test report packaged locally for review; no branch/PR created remotely.
- 2026-10-23 — Owner explicitly authorized publishing the completed dark-blue UI sources to a separate GitHub branch and opening a pull request. Proceeding with publication; main remains unchanged.

- 2026-10-23 — Confirmed PR #1 was merged. At the owner's request, replaced the old README showcase image with the new dark-blue workspace screenshot on main (commit `b93bb9915cf635ffe1aa9692a51b9c7b34d1d6c7`). Verified the remote Markdown and image blob. Documentation-only update; no application code changed.

- 2026-10-23 — Owner requested visible executables/installers in the upcoming release. Prepared release workflow, linked download table/checksum generator, README and beginner instructions on `release/visible-downloads`. actionlint and functional release-note checks passed; actual local RPM and portable archive built and inspected. No release was published.

- 2026-10-23 — Investigated the pasted failure and verified GitHub run/artifact metadata: all Windows/macOS/Linux packages exist; final upload rejected tag `0.1.1` due to the overly strict prefix requirement. Prepared fix, Python regression tests, numeric tag trigger and a provenance-checked recovery workflow on a separate review branch.

- 2026-10-23 — Owner reported sketch circles could not be drawn. Reproduced drag failure (two clicks already worked), added drag creation and exact-radius Properties action, fixed shortcut gesture state, and verified four new app tests plus native drawing/extrusion. Published for review on `fix/sketch-circles`; 72 workspace tests pass.
- 2026-10-23 16:30 — Status audit at the owner's request: re-ran tests on main (72 ok), checked releases/PRs, rewrote Current state + Next steps (docs-only, via branch + PR).
