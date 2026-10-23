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
_Last updated: 2026-10-23 18:40 (feature-tracker push, branch `feat/tracker-stage2`)._

**Read this first. Older details are in the session log and `docs/TEST_REPORT.md`.**
- `main` = PR #5 merged (fillet/chamfer drag handles). Releases: `0.1.1` (prerelease) has
  all packages but predates PR #3–#5; `0.1.0` and `beta` releases are empty. `Cargo.toml`
  version is still 0.1.0 (the RPM name follows it) — bump before the next release.
  AI sessions cannot push tags or use the releases API (HTTP 403): the owner publishes
  releases on github.com and `release.yml` builds/attaches every package.
- **Branch `feat/tracker-stage2` (PR opened 2026-10-23, waiting for the owner)** — finishes
  many tracker items, 102 tests pass locally (fmt + clippy clean):
  - 3MF export (`calibro_io::export_3mf`, Export menu).
  - Smooth edges (cylinder seams) hidden (`kernel::is_smooth_edge`).
  - Section view fills the cut (`kernel::section_cap`, cached per body in `app.cap_cache`).
  - Sketches on faces follow their face (`FaceAttach`, `recompute::resolve_face_attach`,
    `Model::sketch_planes` / `Model::sketch_plane`) → `.calibro` **FORMAT_VERSION 3**.
  - Sketch: crossing curves split into regions (`calibro-sketch/src/intersect.rs`), Trim
    tool (T), centre rectangle, 3-point arc, midpoint/crossing snaps, click a dimension
    label to edit it on the canvas (`dimension_editor` in `viewport.rs`).
  - STEP import as editable solids (`calibro-io/src/step_solid.rs`, fixtures written by
    Open CASCADE in `crates/calibro-io/tests/data`); app falls back to the mesh import when
    a shell doesn't convert. `step_debug_report` (hidden) prints conversion diagnostics.
  - Kernel: sphere meshing bug fixed (spheres containing the origin), `make_torus`,
    `validate_imported`. App: edges behind a face are no longer pickable through it.
- Never launched on real hardware: Windows/macOS binaries, RPM on real Fedora.

## Next steps (in order)
1. Owner: review/merge the `feat/tracker-stage2` PR (check CI green first: wasm build uses
   the new `earcutr` dependency — not compiled for wasm locally).
2. Re-check edge picking on Xvfb after the "edges behind faces" fix (not re-verified).
3. Bump version (0.1.2), update `RELEASE_NOTES.md`, owner publishes the release.
4. STEP import: faces with a seam *and* holes, partial spheres/tori, IGES study.
5. Sketch: extend, offset, tangent arc, splines; constraints for snapped midpoints/crossings.
6. Topological naming for fillet/chamfer edges; corner fillets; shell of curved bodies.
7. Supersampled image export; capped sections with hatch; real-hardware tests.

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

- 2026-10-23 — Owner requested mouse-slider behavior for fillet/chamfer like Shapr3D. Added on-model handles, validated live preview, fine control, Escape cancellation and one-step undo, with Editor transaction support and thirteen regression tests. Native drag/Undo/Esc checks recorded; prepared on a separate review branch.
- 2026-10-23 evening — Owner asked to finish tracker items: 3MF, seam hiding, section caps, sketch-on-face follows face (format v3), sketch intersections + Trim + centre rectangle + 3-point arc + snaps + on-canvas dimensions, STEP → editable solids, sphere bug fix. Branch `feat/tracker-stage2` + PR; owner said "salva e riprendiamo dopo".
