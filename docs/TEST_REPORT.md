# Calibro — Build & Test Report (Stage 1)

Environment: Linux x86_64 sandbox, 2 CPU cores, 7 GB RAM, Rust 1.97.0 (cargo 1.97.0),
software GPU = Mesa llvmpipe/lavapipe (Vulkan), virtual display Xvfb. Date: 22 Oct 2026.

Legend — **Generated**: code written · **Compiled**: builds without errors · **Tested**:
automated test passed · **Manually verified**: exercised in the running app (screenshots) ·
**Not tested** · **Blocked**: could not be done in this environment.

## 1. Commands run and results

| Command | Result |
|---|---|
| `cargo build` (debug, whole workspace) | ✅ Compiled |
| `cargo build --release -p calibro` | ✅ Compiled (see §5 for timing) |
| `cargo fmt --all --check` | ✅ clean |
| `cargo clippy --workspace --all-targets` | ✅ no warnings |
| `VK_ICD_FILENAMES=…/lvp_icd.json cargo test --workspace` | ✅ **58 passed, 0 failed**, 1 ignored (diagnostic) |
| Clean-room check: unzip `calibro.zip` into an empty folder → `cargo test --workspace --locked` | ✅ 58 passed, 0 failed (package is complete; locked dependency versions build) |
| Release binary `target/release/calibro` (31 MB) | ✅ full UI scenario of §3 re-run on it with identical results |
| `cargo run --release -p calibro-kernel --example boolean_probe` | ✅ ran; results in `FEATURE_TRACKER.md` §8 |
| `cargo run -p calibro-io --example export_samples -- <dir>` | ✅ wrote STEP/STL/OBJ/GLB/`.calibro` |
| `cargo run -p calibro-io --example showcase -- <file>` | ✅ 13 features, all OK, 7 bodies |
| `rustup target add wasm32-unknown-unknown` | ⛔ **Blocked**: `static.rust-lang.org` refused by the sandbox network policy (HTTP 403 at the proxy). The browser build therefore could not be compiled here. |

Tests per crate:

| Crate | Tests | What they cover |
|---|---|---|
| calibro-sketch | 17 | regions (rectangle, open polyline, holes, shared edge, arcs, construction, hexagon), solver (dimensions, DOF, conflicts, redundancy, tangent/angle/parallel/perpendicular, equal/concentric/coincident/midpoint/symmetric), dragging, deletion |
| calibro-kernel | 15 | extrude volume, negative/symmetric extrude, holes, arc slot, cylinder/sphere volume, revolve tube, axis-crossing rejection, booleans (subtract/union/intersect), coplanar join/cut with overlap, coplanar booleans never panic, transform, mirror, B-rep JSON round trip |
| calibro-core | 10 | sketch → extrude → edit dimension → downstream update, undo/redo & dirty flag, cut + cache reuse, boolean feature & failure propagation, boss/pocket sketched on a face, rollback, patterns & mirror, delete with dependents, revolve feature, mesh resources, coalesced undo |
| calibro-project | 6 | exact save/load round trip + editability after reload, garbage/foreign ZIP, truncated file, newer version refused, missing resource, atomic save with `.bak` |
| calibro-io | 5 | STL binary/ASCII round trip, OBJ round trip, GLB structure, STEP export → import, STEP refuses reference meshes |
| calibro (app) | 5 (+1 ignored) | camera projection ↔ ray consistency, top view orientation, ray picking incl. section clipping, **GPU rendering** of a box over background, metallic shading not black |

## 2. Independent verification of exported files

Sample part: 60×40×8 mm plate, Ø12 hole, Ø18 boss joined from z = 4…20, Ø8 bore through.
Analytical volume 20 343.5 mm³.

| File | Tool | Result |
|---|---|---|
| `sample.stl` (binary) | trimesh 5.1.1 | ✅ watertight, consistent winding, volume 20 337.38 mm³ (−0.03 %, tessellation), extents 60×40×20 |
| `sample_ascii.stl` | trimesh | ✅ same as binary |
| `sample.obj` + `.mtl` | trimesh | ✅ volume 20 337.38 mm³, extents 60×20×40 (Y-up as intended). Not "watertight" by trimesh's definition because vertices are intentionally split at sharp edges for correct normals |
| `sample.glb` | trimesh | ✅ volume 20 337.38 mm³ after m³→mm³, Y-up, metres |
| `sample.step` | **Open CASCADE via cadquery-ocp 8.0.1** | ✅ read OK, 1 solid, 13 faces, `BRepCheck_Analyzer` valid, exact volume 20 346.66 mm³ (+0.016 %). OCCT logs "Unresolved Reference" warnings for 32 auxiliary entities; geometry is unaffected. Boolean intersection curves are written as polylines by truck, which explains the small deviation |

Not done: opening the files in Blender, Fusion, Maya, 3ds Max or Cinema 4D (not available here).
**Manual procedure:** Blender → *File → Import → glTF 2.0 / Wavefront OBJ / STL*; Fusion →
*Upload/Open* the STEP; check dimensions with the measure tool (60 × 40 × 20 mm).

## 3. Running application (manually verified on Xvfb + software Vulkan)

| Scenario | Result | Evidence |
|---|---|---|
| App starts, viewport renders grid/axes | ✅ | first launch |
| New sketch → Top → rectangle + circle; H/V auto constraints; DOF = 7; 2 regions | ✅ | `docs/images/sketch.png` |
| Finish sketch → click ring region → E → extrude with through hole | ✅ | |
| Drag the extrude handle: 10 → 21.5 mm live | ✅ (after fixing a drag-start bug found in this test) | |
| Select top face → *Sketch on this face* → circle → E → joins (still 1 body) | ✅ (after fixing a coplanar boolean failure found in this test) | `docs/images/boss-on-face.png` |
| Box overlapping with coplanar bottom → Subtract | ✅ reported as a failed feature with a clear message; app keeps running (it **crashed** before the panic guard was added) | `docs/images/kernel-error-reported.png` |
| Edit Box 1 corner Z = −2 → Subtract rebuilds automatically | ✅ parametric fix works | `docs/images/parametric-edit.png` |
| Measure tool with vertex snapping: 51.000 mm, ΔX/ΔY/ΔZ | ✅ | |
| Section view along X | ✅ (no caps yet) | `docs/images/section-view.png` |
| Select tool: select a line → *Length* → type 80 mm → rectangle resizes, DOF 4 → 3 | ✅ | `docs/images/sketch-dimension.png` |
| Drag a constrained corner: shape stays rectangular, width stays 80 mm | ✅ (after fixing the same drag-start bug) | `docs/images/sketch-drag.png` |
| Undo ×4 back to empty document; keyboard shortcuts | ✅ | |
| Open project from command line (`calibro examples/showcase.calibro`) | ✅ 13 features rebuilt | `docs/images/showcase.png` |
| Materials (aluminium, brass, red plastic) | ✅ (after fixing a colour-space bug found in this test) | |
| File dialogs (Open/Save/Export/Import) | ⚠️ **Not tested** — need a desktop session with a file-chooser portal; the underlying save/load/export code is unit-tested | |
| Window close with unsaved changes | ⚠️ Not tested in UI (logic unit-tested) | |
| Touch / pen input | ⚠️ Not tested (no touch device) | |

## 4. Bugs found by testing and fixed in this session

1. Integer-keyed sketch maps lost after JSON round trip inside tagged enums → serialized as entry lists.
2. Holes made from circles were filled instead of cut (wrong wire orientation) → inner circles reversed.
3. Exported meshes not watertight (boundary vertices differing in the last bits) → tolerant weld for export.
4. Drag handles and sketch-point dragging ignored (egui reports drag start after movement) → hit-test at press origin.
5. Joining a boss sketched on a face failed (truck can't intersect coplanar faces) → overlap retry.
6. **Kernel panic in a boolean crashed the app** → every kernel call isolated with `guarded()`; results validated.
7. Viewport colours too dark (sRGB texture sampled by egui as gamma data) → render to `Rgba8Unorm`, encode gamma in the shader; verified with GPU pixel tests.
8. Text glyphs missing for icon characters → replaced with text labels.

## 5. Not verified / blocked

- **WebAssembly build — Blocked here** (toolchain download refused). Static review done:
  `getrandom` `js` feature added, WebGL2 texture size clamped, web-only APIs checked against
  the installed `web-sys`/`eframe` sources. The CI `web` job and the Pages workflow will compile
  it; treat the browser version as unverified until they are green.
- **GitHub Actions workflows** — written, not run (no repository yet).
- **Windows and macOS builds** — not compiled here; configured in `release.yml`.
- Performance with large models — not measured.

## 6. Update — 23 Oct 2026 (first GitHub runs)

| Item | Result |
|---|---|
| Pages workflow (WebAssembly build + deploy) | ✅ success on GitHub; site live |
| Web app in a real browser (Claude desktop browser pane) | ✅ starts, renders the viewport, creates a Box (kernel works in wasm) |
| Release workflow (manual run) | ✅ Linux 9.96 MB, macOS arm64 7.9 MB, Windows 8.62 MB artifacts |
| CI | ❌ Clippy step failed on GitHub (stable = Rust 1.99.1, newer lints than the tested 1.97.0) → fixed by pinning `rust-toolchain.toml` to 1.97.0 in all workflows |
| Issues found in the browser and fixed | window title re-sent every frame (console flooded) → only when it changes, `document.title` on web; side panels squeezed the viewport on narrow screens → compact widths, auto-hide below 900/620 px, *Tools*/*Panel* toggles; view bar overlapped the inspector → wraps within the viewport |
| After the fixes (local, Rust 1.97.0) | ✅ fmt, clippy `-D warnings`, 58 tests, release build, UI toggles checked on Xvfb |

## 7. Update — 23 Oct 2026 (Modify tools, sketch-on-face with Space)

| Item | Result |
|---|---|
| New kernel ops (`calibro-kernel/src/ops.rs`): fillet, chamfer, shell, sweep, loft | ✅ 6 new kernel tests (volumes checked against analytic values; refusals checked) |
| New features in history + format v2 (`Fillet`, `Chamfer`, `Shell`, `Sweep`, `Loft`) | ✅ core tests `modify_features_fillet_chamfer_shell` (incl. parametric thickness edit, shell after chamfer, edge that disappears → clear failure) and `sweep_and_loft_features`; project test `format_v1_files_still_open` + round trip with the new features |
| Full suite (Rust stable locally, lavapipe) | ✅ fmt, clippy `-D warnings`, **68 passed**, 1 ignored |
| UI on Xvfb: box → click edge, Shift+click a second edge → *Chamfer* → both edges bevelled; click top face → *Shell* → open tray | ✅ `docs/images/shell-chamfer.png` (found and fixed: shell refused chamfered boxes because of the tool's hair-thin sliver faces) |
| UI: box → *Fillet* on one edge | ✅ |
| UI: click a side face → **Space** → sketch starts on that face, ortho camera looks straight at it, face centred and framed | ✅ |
| UI: `examples/modify-tools.calibro` (tray, swept pipe, loft) opened from the command line; Sweep inspector with path selector | ✅ `docs/images/modify-tools.png` |
| UI: creating Sweep/Loft by clicking regions and the toolbar buttons | ⚠️ Not clicked through on Xvfb (command code reviewed; the features themselves are tested) |
| Note for testers | xdotool keys/modifiers only reach the window after `xdotool windowfocus` (no window manager on Xvfb) |
