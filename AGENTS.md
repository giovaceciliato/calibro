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
_Last updated: 2026-10-23._
- Stage 1 complete (see tracker). Web build verified live in a real browser; Windows/macOS/Linux
  binaries built on GitHub (manual run, not yet a Release).
- First CI run failed at Clippy because GitHub used Rust 1.99.1 → fixed locally by pinning 1.97.0.
- Fixes made on 2026-10-23 but **not yet on GitHub** at the time of writing (packaged as
  `calibro-update.zip` for the owner, and/or to be pushed once GitHub access works):
  `rust-toolchain.toml`, the three workflows (pinned toolchain, upload-artifact@v7,
  action-gh-release@v3), responsive panels + Tools/Panel toggles + title fix in
  `app/src/{app,panels,viewport}.rs`, `RELEASE_NOTES.md`, docs updates, this file.

## Next steps (in order)
1. Get the 2026-10-23 changes onto `main` (push, or the owner uploads them). Confirm CI is green.
2. With the owner's OK: create release `v0.1.0` (tag on `main`, body = `RELEASE_NOTES.md`);
   check that `release.yml` attaches the three archives.
3. Re-check the live web app after deploy (narrow-screen layout, Tools/Panel toggles).
4. Stage 2 (tracker §9): on-canvas dimension editing, trim/extend + curve intersection,
   topological naming v1 for sketches on faces, planar chamfer, shell, two-profile loft,
   3MF export, hide cylinder seam edges, capped section view.

## Session log
- 2026-10-22 — Stage 1 built: research, architecture, all crates, app, native format, exports,
  CI/Pages/release workflows, docs. 58 tests passing. Desktop UI verified on Xvfb.
- 2026-10-23 — Owner uploaded to GitHub. Pages deploy and desktop builds succeeded; CI failed
  at Clippy (Rust 1.99). Pinned toolchain, made layout responsive, fixed title spam, wrote
  release notes, added this handoff file.
