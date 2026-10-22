# Calibro — Research Report

Date: 22 October 2026. Every claim below is either cited, measured in this repository, or
marked as an assumption. Versions are the ones resolved by Cargo on that date.

## 1. What Shapr3D does (the reference)

Shapr3D is a commercial, history-based parametric CAD application for iPad, Mac, Windows and
Vision Pro. From its public pages:

- **Sketching**: line, arc, circle, rectangle, spline, ellipse and polygon tools; pattern and
  mirror of sketch elements; geometric constraints (parallel, perpendicular, tangent,
  coincident, midpoint, concentric, horizontal/vertical, equal, symmetry); auto-constraints
  while drawing; dimensional constraints with typed values; colour-coded fully / under / over
  defined sketches. ([CAD constraints page](https://shapr3d.com/product/cad-constraints))
- **Modelling**: extrude, chamfer, fillet, loft, sweep, union/subtract/intersect, shell,
  split, wrap & emboss, replace face, offset, linear and circular patterns, project, direct
  push-pull modelling *and* parametric history; an adaptive toolbar that suggests tools for
  the current selection. ([3D modeling page](https://www.shapr3d.com/product/3d-modeling))
- **Parametric history**: features can be edited after the fact and later features update.
  ([History-based parametric modeling](https://www.shapr3d.com/blog/shapr3d-history-based-parametric-modeling))
- **Extrude details**: profile/face extrusion with join/cut behaviour and an adaptive UI.
  ([The Extrude tool](https://support.shapr3d.com/hc/en-us/articles/26649987200796),
  [Extrude with adaptive UI](https://support.shapr3d.com/hc/en-us/articles/13079186051996))
- **Loft/Sweep and Revolve/Shell workflows** are documented in tutorials.
  ([Loft and Sweep](https://support.shapr3d.com/hc/en-us/articles/19202990944284),
  [Revolve, Shell, Sweep](https://support.shapr3d.com/hc/en-us/articles/15984985933596))
- **Files**: native `.shapr`; imports STEP, IGES, X_T/X_B, SLDPRT/SLDASM, STL (reference
  only), DWG/DXF sketches, images and PDF; exports SHAPR, X_T/X_B, STEP, 3MF, IGES, OBJ (with
  colours), STL, GLB, USDZ and 2D DWG/DXF/PDF/SVG/PNG/JPEG; CATIA/NX/Creo/JT need Enterprise.
  ([Supported file types](https://support.shapr3d.com/hc/en-us/articles/7874523890076))
- **Visualisation**: materials, environments, AI renders, AR, immersive Vision Pro view.
  ([3D modeling page](https://www.shapr3d.com/product/3d-modeling))

The internal `.shapr` format and Shapr3D's kernel are proprietary and undocumented. Shapr3D
licenses the Parasolid kernel (widely reported; not verified from a primary source here, so
treat as background only). Calibro does **not** read or write `.shapr`.

The full feature-by-feature inventory with status is in `FEATURE_TRACKER.md`.

## 2. Geometry kernel

| Criterion | **truck** (chosen) | **opencascade-rs** (OCCT bindings) | **Fornjot** |
|---|---|---|---|
| Implementation | Pure Rust | Rust bindings to C++ Open CASCADE | Pure Rust |
| Representation | B-rep: NURBS/B-spline curves & surfaces, topology (vertex…solid) | Full OCCT B-rep | B-rep (early) |
| Solid modelling | Yes (tsweep, rsweep, homotopy, builders) — **measured working** | Yes in OCCT; Rust API partial | Basic |
| Booleans | `truck-shapeops`: union/subtract/intersect — **work for planar & cylindrical faces; fail for spheres and coplanar faces** (measured, `boolean_probe`) | OCCT booleans are mature; README lists high-level API as "in flux" | Not a focus |
| Fillet / chamfer / shell | Not in released 0.6 crates | In OCCT; bindings list them as goals | No |
| Parametric suitability | Good: fast, deterministic, serializable (`serde`) shapes | Good (OCCT has OCAF) | Model-as-code |
| Sketching | Builders for lines/arcs/B-splines; no constraint solver | No solver in bindings | No |
| Tessellation | `truck-meshalgo` — **measured 0.03 % volume error** | OCCT BRepMesh | Yes |
| CAD exchange | `truck-stepio` STEP out + in — **STEP output read as a valid solid by Open CASCADE** | OCCT STEP/IGES (bindings list STEP as goal) | No |
| WebAssembly | Yes in principle (project ships `truck-js`); Calibro's wasm build not yet run | Not available (README: future work) | Yes |
| Desktop | Linux/Windows/macOS (pure Rust) | Needs CMake + C++ toolchain per platform | Yes |
| Documentation | API docs + tutorials (v0.6) | Early; examples only | Docs |
| Maintenance | crates 0.6.0 published Sept 2024; repo active (2,870 commits, 1.5k stars at fetch time) | crate `opencascade` 0.3.0 Aug 2026; README: "major work in progress" | `fj-core` 0.49.0 Mar 2024 |
| Licence | Apache-2.0 | LGPL-2.1 (OCCT: LGPL-2.1 with an additional exception) | Permissive (not re-verified for this report) |
| Integration effort | Low (cargo only) — **done** | High (C++ build, binding gaps, no web) | Medium, too immature |

Sources: [truck repository](https://github.com/ricosjp/truck), [opencascade-rs repository](https://github.com/bschwind/opencascade-rs), crates.io API (versions/dates), measurements in this repo (`docs/TEST_REPORT.md`).

**Decision: truck**, accessed only through the `calibro-kernel` crate. It keeps the project
pure Rust, permissively licensed, buildable for the browser, and it already provides exact
B-rep extrude/revolve/booleans/tessellation/STEP. Its gaps (fillet, chamfer, shell, robust
booleans on curved/coplanar geometry) are real and documented; the adapter boundary allows a
second backend (for example OCCT on desktop) later without touching the rest of the app.

**Constraint solver**: no maintained, permissively licensed Rust 2D constraint solver was
found suitable (SolveSpace's solver is GPL-3 C++). Calibro implements its own:
Levenberg–Marquardt on residual equations with numerical Jacobians, rank-based degrees of
freedom and conflict/redundancy detection (`calibro-sketch/src/solver.rs`).

## 3. Rendering

**wgpu** ([repository](https://github.com/gfx-rs/wgpu), v30.0.1) gives one API over Vulkan,
Metal, DirectX 12, OpenGL, WebGPU and WebGL2. Calibro renders the viewport offscreen (4× MSAA,
32-bit depth) and hands the image to egui. Implemented: shaded/edges/wireframe/X-ray modes,
studio hemisphere + key/fill lighting, metallic/roughness materials with a procedural
environment for reflections, face/body/edge highlighting, translucent region fills, grid,
section clipping. Measured: rendering tests on a software GPU (Mesa lavapipe) check that a box
covers the image centre and that matte and metallic top faces are lit correctly.

Separation of concerns (spec §6.3): exact `Solid` (kernel) → `BodyMesh` (tessellation, cached
by content hash) → GPU buffers (re-uploaded only when the mesh `Arc` changes) → picking uses
the mesh but maps hits back to B-rep face ids and exact face planes.

Not yet: shadows, ambient occlusion, HDR image-based lighting, capped sections, path-traced
"presentation" renders (would be a separate renderer, e.g. a progressive path tracer).

## 4. User interface

**egui / eframe** ([repository](https://github.com/emilk/egui), v0.37.0): immediate-mode,
runs natively and in the browser, integrates directly with wgpu, has panels, menus, modals,
drag values, colour pickers, keyboard shortcuts and touch events (`multi_touch`). It provides
no CAD interaction model, so Calibro implements its own: tools, snapping, inference,
selection priorities (edges > sketch regions > faces), handles and gesture-merged undo.

## 5. Platform architecture

| Option | Pros | Cons |
|---|---|---|
| Browser only | Zero install, GitHub Pages hosting | No real file system, memory limits, single-threaded kernel, WebGL2 limits |
| Desktop only | Full performance and file access | Needs installers; no instant link to share |
| **Hybrid (chosen)** | One Rust codebase; desktop for serious work, browser for instant access | Two build targets to test |

Everything except `app/src/files.rs`, `main.rs` and a few `#[cfg]` lines is platform-neutral.
The kernel compiles to WebAssembly in principle (truck publishes a JS/wasm package), so the
hybrid choice does not compromise the long-term goal.

## 6. File formats

| Category | Formats in Calibro | What survives |
|---|---|---|
| Native project | `.calibro` (ZIP: `mimetype`, `manifest.json`, `document.json`, `resources/`) | Everything: history, sketches, constraints, parameters, bodies' names/materials/visibility, camera, units, metadata |
| CAD exchange | STEP (export exact, import as reference mesh) | Exact geometry only; no history/constraints/materials |
| Mesh | STL (binary/ASCII), OBJ+MTL | Triangles (+ normals, colours for OBJ) |
| Scene | glTF binary (`.glb`) | Meshes, PBR materials, body names, hierarchy; metres, Y-up |

Why ZIP+JSON for `.calibro`: robust and inspectable (any unzip tool), extensible with binary
resources and thumbnails, fast enough for documents of this size, and the same container
approach as 3MF/DOCX. The format is versioned (`format_version`), files from newer versions are
refused with a clear message, and a `migrate()` hook exists for older versions.

FBX: no reliable pure-Rust FBX writer exists; Blender, Maya, 3ds Max and Cinema 4D all import
glTF/GLB, so GLB is recommended instead. IGES: no maintained Rust library found; would require
writing an exporter (planned).

## 7. Licensing (not legal advice)

| Dependency | Licence | Obligation |
|---|---|---|
| truck-* | Apache-2.0 | Keep licence & notices |
| egui / eframe / egui-wgpu | MIT OR Apache-2.0 | Attribution |
| wgpu | MIT OR Apache-2.0 | Attribution |
| zip, rfd, png, glam, serde, bytemuck | MIT and/or Apache-2.0 | Attribution |

All are permissive and compatible with **Apache-2.0** for Calibro itself (chosen because it
matches truck and includes a patent grant; MIT would also be compatible). If an Open CASCADE
backend is ever added, its LGPL-2.1 terms (dynamic linking or providing relinkable objects)
must be reviewed before distributing binaries. To produce a full third-party licence list for
releases, run `cargo install cargo-about && cargo about generate` (recommended before the first
public release).

Intellectual property: Calibro is independently implemented. It uses no Shapr3D code,
assets, icons or file format, and it is not presented as a Shapr3D product. Using
"Shapr3D-like" in descriptions is fine; using Shapr3D's name or logo as the product's identity
is not. A review by a lawyer is advisable before any commercial use.

## 8. Risks

1. **Kernel robustness** (high): truck booleans fail on spheres and coplanar faces — measured.
2. **Fillets** (high): Shapr3D users rely on them; needs a kernel strategy decision.
3. **Topological naming** (medium-high): needed for robust parametric edits of face-based sketches.
4. **Web build unverified** (medium): blocked in this sandbox; the CI job will reveal issues.
5. **Panics on WebAssembly abort the app** (medium): truck release builds skip the asserting checks; results are validated.
6. **Solver scale** (low-medium): dense LM is fine for typical sketches, slow for very large ones.
