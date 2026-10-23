# Calibro — Master Feature Tracker

Last updated: 22 October 2026 · Stage 1 (foundation) complete · Next: Stage 2

This tracker covers the **complete intended scope** (a full Shapr3D-class CAD app), what
**works today**, and what **remains**. It is the continuation point for future sessions.

**Status values** (as required by the specification):
`Not researched` · `Researched` · `Planned` · `In progress` · `Implemented` · `Tested` · `Blocked`

- **Tested** = covered by an automated test that passed *and/or* exercised in the running app
  (see `docs/TEST_REPORT.md` for evidence). Nothing is marked Tested on the basis of code alone.
- **Implemented** = code exists and compiles, but has no dedicated test yet.

**Evidence sources for Shapr3D features** (public pages, accessed 22 Oct 2026):
- [S1] Shapr3D product page, "Easy 3D modeling tools" — https://www.shapr3d.com/product/3d-modeling
- [S2] Shapr3D product page, "CAD constraints" — https://shapr3d.com/product/cad-constraints
- [S3] Shapr3D Help Center, "Supported file types" — https://support.shapr3d.com/hc/en-us/articles/7874523890076
- [S4] Shapr3D blog, history-based parametric modeling — https://www.shapr3d.com/blog/shapr3d-history-based-parametric-modeling
- [S5] Shapr3D Help Center, "The Extrude tool" — https://support.shapr3d.com/hc/en-us/articles/26649987200796
- [S6] Shapr3D Help Center, "Fast concept development with Loft and Sweep" — https://support.shapr3d.com/hc/en-us/articles/19202990944284
- [S7] Shapr3D Help Center, "Organic chair design with Revolve, Shell, and Sweep" — https://support.shapr3d.com/hc/en-us/articles/15984985933596

Where behaviour of a proprietary feature is not publicly documented, the "Calibro approach"
column says how we approximate it.

---

## 1. Summary

| Area | State at the end of Stage 1 |
|---|---|
| Sketching & constraints | Core tools, all listed constraint types and dimensions, solver and region detection work and are tested. Splines, ellipses, trim/extend, sketch patterns and DXF import remain. |
| Solid modelling | Extrude, revolve, booleans, primitives, transforms, patterns and mirror work and are tested. Fillet, chamfer (straight edges between flat faces), shell (convex flat-faced bodies), sweep (smooth line/arc paths) and loft (two profiles) added 2026-10-23 with limits; direct modelling remains. |
| History / parametric | Feature history, dependency-driven recompute, caching, failure propagation, undo/redo work and are tested. Topological naming remains. |
| Viewport, rendering, interaction | wgpu viewport, navigation, display modes, materials, selection, section view, measuring and handles work. Shadows, AO, HDR lighting, capped sections remain. |
| Files & interoperability | Native `.calibro`, STEP export, STL/OBJ/GLB export and STEP/STL/OBJ import (as reference meshes) work and are tested, partly with independent tools. IGES, 3MF, USDZ remain; proprietary formats are blocked. |
| Platform, build, deployment | Native Linux build verified; Windows/macOS/web built on GitHub; web app live on GitHub Pages and checked in a real browser. Responsive layout for narrow screens added. |

The application is a **working foundation**, not a finished Shapr3D equivalent. The largest
gaps are general fillet/chamfer/shell (curved edges, corners, non-convex bodies — kernel-limited), topological naming (sketches on
faces don't follow later edits of that face), sphere/torus booleans (kernel-limited), 2D
drawings, and Parasolid/native CAD imports.

---

## 2. Sketching, dimensions and constraints

| Feature | Shapr3D evidence | Calibro status | Test | Where | Limitations · next action |
|---|---|---|---|---|---|
| Sketch on standard planes (Top/Front/Right) | [S1][S2] | Tested | unit + UI | `app/src/app.rs` `start_sketch` | — |
| Sketch on a planar face of a body | [S1] | Tested | UI (sketch on top face) | `viewport.rs` plane pick, `panels.rs` | Plane is captured when the sketch is made; it does not follow later edits of that face (topological naming). Next: store face reference + re-resolve. |
| Line (chained polyline, closes on first point) | [S2] | Tested | UI | `viewport.rs` `sketch_input` | — |
| Rectangle (with automatic H/V constraints) | [S2] | Tested | unit + UI | `calibro-sketch/model.rs` | Centre-rectangle variant: planned |
| Circle | [S2] | Tested | four app input/command tests + native UI, report §11 | `create_sketch_circle`, `viewport.rs interaction_tests` | Centre + circumference clicks, centre-drag-release, exact radius in Properties; undo/redo and cancellation checked |
| Arc (centre, start, end) | [S2] | Tested | unit (arc regions + extruded slot) | | 3-point and tangent arcs: planned |
| Regular polygon | [S2] | Tested | unit (hexagon area exact) | | — |
| Spline | [S2] | Planned | — | | Needs B-spline entity + solver handles; truck supports B-splines |
| Ellipse | [S2] | Planned | — | | |
| Sketch Pattern / Mirror of entities | [S2] | Planned | — | | |
| Construction geometry | spec | Tested | unit | `Curve.construction` | — |
| Point snapping (existing points, origin, grid) | [S2] auto-constraining | Tested | UI | `viewport.rs` | Midpoint/intersection snaps: planned |
| Auto-constraints while drawing (H/V) | [S2] | Tested | UI ("H" glyphs) | | Auto perpendicular/tangent/coincident-on-curve: planned |
| Coincident · Horizontal · Vertical · Parallel · Perpendicular | [S2] | Tested | unit | `solver.rs` | — |
| Tangent (line–circle/arc, circle–circle) | [S2] | Tested | unit | | — |
| Equal (length / radius) · Concentric · Midpoint | [S2] | Tested | unit | | — |
| Symmetric | [S2] | Tested | unit | | — |
| Fixed | spec | Tested | unit | | — |
| Dimensions: distance, H/V distance, length, angle, radius, diameter | [S2] | Tested | unit + UI | | On-canvas dimension editing (click label to type): planned; values are edited in the inspector |
| Constraint solver (Levenberg–Marquardt, DOF analysis) | [S2] states | Tested | unit (15) | `solver.rs` | Large sketches (>~300 entities) will be slow (dense solver). Next: sparse solver |
| Fully / under / over-defined status, colours | [S2] | Tested | unit + UI | `panels.rs status_text` | Shapr3D colours curves green/blue; Calibro does the same |
| Conflicting constraint rejection | [S2] | Tested | unit | `add_sketch_constraint` | — |
| Drag points with constraints kept | spec | Tested | unit | `drag_point` | — |
| Closed-region detection (holes, shared edges, arcs) | [S5] (extrude profiles) | Tested | unit (6) | `profile.rs` | Curves that cross without a shared endpoint are not split. Next: curve–curve intersection |
| Trim / extend / offset / fillet in sketch | not verified in sources | Researched | — | | Planned for Stage 3 |
| Project edges onto sketch | [S1] "Project" | Planned | — | | |
| Import DWG/DXF into sketches | [S3] | Planned | — | | DXF is feasible in pure Rust; DWG is proprietary (Blocked-ish) |

## 3. Solid modelling

| Feature | Shapr3D evidence | Calibro status | Test | Where | Limitations · next action |
|---|---|---|---|---|---|
| Extrude (distance, flip, symmetric) | [S1][S5] | Tested | unit + UI | `kernel::extrude`, `recompute.rs` | "Up to face/body" extents: planned |
| Extrude result modes: new body / join / cut / intersect | [S5] | Tested | unit + UI | `BodyMode` | — |
| Automatic join ↔ cut when pushing a face sketch into / out of its body (Shapr3D behaviour) | [S5] | Tested | unit + UI (red "cut" handle) | `Document::auto_extrude_mode` | Only when the direction flips, so a mode chosen manually in the inspector is kept |
| Join/cut from a sketch lying exactly on a face | [S5] | Tested | unit + UI | `extrude_with_overlap` | Workaround for a truck limitation (coplanar faces): tool is extended 0.2 mm into the body when the exact boolean fails |
| Extrude drag handle in viewport | [S5] adaptive UI | Tested | UI | `viewport.rs handles` | — |
| Revolve (sketch X/Y axis or a sketch line, any angle) | [S1][S7] | Tested | unit | `kernel::revolve` | — |
| Box / Cylinder / Sphere primitives | — (Calibro convenience) | Tested | unit + UI | | — |
| Boolean union / subtract / intersect | [S1] | Tested | unit + UI + probe | `kernel::boolean` | Works for planar & cylindrical faces. **Fails for spheres / revolved surfaces with poles and for exactly coplanar faces** (truck 0.4). Failure is reported, never crashes (native). See §8 |
| Move / rotate / scale bodies (exact values) | spec | Tested | unit | `Transform` feature | Gizmo arrows translate only; rotate ring: planned |
| Linear pattern | [S1] | Tested | unit | | — |
| Circular pattern | [S1] | Tested | unit + showcase | | — |
| Mirror body | spec | Tested | unit | `kernel::mirror` | — |
| Fillet | [S1] | **Tested** (limited) | `fillet_box_edge`, `modify_features_fillet_chamfer_shell`; drag tests + Xvfb (report §12) | | On-model radius handle with live preview, Shift fine control, Esc cancel. Own implementation (`calibro-kernel/src/ops.rs`): straight edges between two flat faces, any angle; several edges at once. Not yet: edges meeting at a corner (refused with a message), curved edges, variable radius. The round's centre is offset 0.5 % of r so truck's boolean stays transversal. General case: optional Open CASCADE backend or upstream truck work |
| Chamfer | [S1] | **Tested** (limited) | `chamfer_two_edges_and_reject_bad_edges`; drag tests + Xvfb (report §12) | | Equal-distance chamfer on straight edges between flat faces (wedge cut); on-model size handle with live preview, Shift fine control, Esc cancel. Leaves hair-thin slivers (0.2 % of the distance) because of the boolean nudge |
| Shell | [S1][S7] | **Tested** (limited) | `shell_open_box`, core test; UI on Xvfb | | Convex bodies with flat faces only (incl. chamfered ones); one or more open faces or closed hollow; editable thickness. Curved / non-convex bodies refused with a message (needs surface offset) |
| Sweep | [S1][S6] | **Tested** (limited) | `sweep_along_line_and_arc`, `sweep_and_loft_features`; `examples/modify-tools.calibro` opened in the UI | | Profile regions along a path sketch of connected lines + tangent arcs (open chain) that starts at the profile. Sharp corners refused; no twist/scale; no splines |
| Loft | [S1][S6] | **Tested** (limited) | `loft_square_to_square_and_circle`, core test | | Two closed profiles without holes. Exact ruled surfaces when segment counts match, otherwise resampled to 48-point polygons (shows many edge lines). No guide rails, no more than two profiles |
| Split body / Wrap & Emboss / Replace face / Offset face | [S1] | Researched | — | | Stage 4+ |
| Push-pull direct modelling of faces | [S1] | Planned | — | | Needs face-level history entries |

## 4. History and parametric modelling

| Feature | Evidence | Calibro status | Test | Where | Limitations · next action |
|---|---|---|---|---|---|
| Feature history (ordered list of features) | [S4] | Tested | unit + UI | `document.rs` | — |
| Edit any earlier feature → downstream recomputes | [S4] | Tested | unit + UI (box Z fixed failed subtract) | `recompute.rs` | — |
| Dependency graph (feature → features) | spec | Tested | unit | `feature_dependencies`, `dependents` | — |
| Failure states with reasons, downstream propagation | spec | Tested | unit + UI | `FeatureStatus` | — |
| Result caching (content-hash per feature) | spec | Tested | unit (`cut_with_cylinder_and_cache_reuse`) | `Engine` | — |
| Suppress / rollback marker | spec | Tested | unit (rollback) | | Suppress has UI but no dedicated test |
| Undo / redo (whole document, gesture-merged) | spec | Tested | unit + UI | `editor.rs` | — |
| Delete feature with dependents | spec | Tested | unit | | — |
| Topological naming (references to faces/edges survive edits) | implied by [S4] | Planned | — | | Hard problem; Stage 3 design item |

## 5. Viewport, rendering and interaction

| Feature | Evidence | Calibro status | Test | Where | Limitations · next action |
|---|---|---|---|---|---|
| wgpu viewport (offscreen MSAA 4×, depth) | spec | Tested | GPU test + UI | `renderer.rs` | — |
| Orbit / pan / zoom-to-cursor, pinch & touch | spec | Tested | UI (mouse); touch implemented | `camera.rs`, `viewport.rs` | Touch/stylus not tested on a real tablet |
| Perspective / orthographic, standard views, fit | spec | Tested | unit + UI | | View cube: planned (buttons used instead) |
| Shaded+edges / shaded / wireframe / X-ray modes | spec | Tested | UI | | — |
| Studio lighting, PBR-style metallic/roughness | [S1] materials | Tested | GPU test + UI | `viewport.wgsl` | Image-based lighting from HDR environments: planned |
| Materials (presets, colour, metallic, roughness, opacity) | [S1] | Tested | UI | `material_presets` | Textures: planned |
| Face / body / edge selection, hover highlight | spec | Tested | unit (picking) + UI | `picking.rs` | Vertex selection only via measure snapping |
| Sketch regions shown filled, hover/selected colours | Shapr3D-like | Tested | UI | | — |
| Section view (X/Y/Z, offset, flip) | spec | Tested | UI | shader clip plane | Cut faces are not capped (hollow look). Next: stencil caps |
| Measure distance (with vertex snapping), ΔX/ΔY/ΔZ | spec | Tested | UI | | Angle/radius measure: planned |
| Body volume, surface area, size; face area; edge length | spec | Tested | UI | | From tessellation (≈0.03 % on test part) |
| Drag handles (extrude distance, move arrows) | [S5] | Tested | UI (extrude) | | Move arrows implemented, not UI-tested |
| Keyboard shortcuts | [S1] | Tested | UI (several) | `app.rs SHORTCUTS` | Customisation: planned |
| Shadows, ambient occlusion, reflections of other bodies | spec | Planned | — | | Stage 5 rendering work |
| High-quality image export (PNG of viewport) | [S1] renders | Implemented | GPU readback tested; dialog not testable here | `save_image` | Desktop only; supersampled export: planned |
| Photoreal rendering / AI renders / AR / Vision Pro | [S1] | Researched | — | | Out of scope for now; AR needs native mobile |
| Seam edges on cylinders hidden | Shapr3D-like | Planned | — | | Cylinder seams are drawn as edges today |

## 6. Files and interoperability

| Feature | Evidence | Calibro status | Test | Where | Limitations |
|---|---|---|---|---|---|
| Native `.calibro` project (ZIP: manifest + document + resources) | [S3] `.shapr` equivalent | Tested | unit (6) + app CLI open | `calibro-project` | — |
| New / Open / Save / Save as | spec | Implemented (UI), Tested (logic) | unit; desktop dialogs not testable in sandbox | `app.rs` | File dialogs need a desktop session to verify |
| Save/load round-trip keeps full editability | spec | Tested | unit | | — |
| Unsaved-change tracking, warning on close/new/open | spec | Tested (logic) | unit (dirty flag) | `editor.rs`, `dialogs()` | Dialog shown in UI; close-window path not tested here |
| Invalid / truncated / newer-version file handling | spec | Tested | unit | | — |
| Atomic save with `.bak` backup | spec | Tested | unit | `save_to_path` | — |
| Crash recovery (autosave of unsaved work every 20 s) | spec | Implemented | — | `eframe::App::save` | Imported meshes are not included in the recovery copy |
| Open file from command line / "Open with" | — | Tested | UI | `CalibroApp::new` | — |
| STEP export (exact B-rep) | [S3] | Tested | unit + **Open CASCADE reads it: valid solid** | `export_step` | Boolean intersection curves are written as polylines (tiny deviation, 0.016 % volume) |
| STEP import | [S3] | Tested | unit (re-import own STEP) | `import_step` | Imported as **reference mesh**, not editable B-rep yet |
| STL export binary/ASCII (watertight) | [S3] | Tested | unit + **trimesh: watertight** | | — |
| STL import | [S3] (reference only) | Tested | unit | | Reference mesh |
| OBJ + MTL export (Y-up, colours) | [S3] | Tested | unit + trimesh | | — |
| OBJ import | — | Tested | unit | | Reference mesh |
| glTF binary (.glb) export (PBR, metres, Y-up) | [S3] | Tested | unit + trimesh | | Textures: n/a yet |
| Export selected bodies vs all visible | spec | Implemented | — | | — |
| IGES import/export | [S3] | Planned | — | | No maintained pure-Rust IGES library found; would need own writer |
| 3MF export | [S3] | Planned | — | | Simple XML+ZIP; good next format |
| USDZ export | [S3] | Planned | — | | |
| FBX | spec | Researched | — | | No reliable Rust FBX writer; recommend GLB instead (Blender, Maya, C4D, 3ds Max all read glTF) |
| Parasolid X_T/X_B, SLDPRT, CATIA, NX, Creo, JT | [S3] | **Blocked** | — | | Proprietary formats/kernels; licensing required |
| DWG/DXF 2D export, 2D drawings | [S1][S3] | Planned | — | | Stage 4 |

## 7. Platform, build and deployment

| Item | Status | Test | Notes |
|---|---|---|---|
| Native desktop build | Tested (Linux run); Windows/macOS compiled on GitHub | Linux run on Xvfb with software Vulkan | Windows/macOS binaries not yet launched on real machines |
| Browser build (WebAssembly) | Tested | built by GitHub Actions, published on GitHub Pages, opened in a real browser: app starts, viewport renders, box primitive created | Live at https://giovaceciliato.github.io/calibro/ |
| GitHub Actions CI (fmt, clippy, tests incl. GPU tests, wasm) | Implemented | ran on GitHub: wasm job green; clippy failed on Rust 1.99 → toolchain now pinned to 1.97.0 (`rust-toolchain.toml`) | `.github/workflows/ci.yml` |
| GitHub Pages deployment | Tested | deployed successfully | `.github/workflows/pages.yml` |
| Desktop release packages on version tags | Tested (manual run) | Windows, macOS arm64 and Linux builds succeeded on GitHub (8–10 MB each) | `.github/workflows/release.yml` |
| Kernel panic isolation | Tested (native) | unit | On WebAssembly panics abort; release truck builds skip the asserting checks |
| iPad / touch-first UI | Planned | — | eframe runs in iPad Safari via the web build; native iPad app would need a different shell |
| Cloud sync, team spaces, review links | Researched | — | Out of scope for an open-source local app |

## 8. Known kernel limitations (truck 0.6 / shapeops 0.4) — measured, not assumed

From `cargo run --release -p calibro-kernel --example boolean_probe` (22 Oct 2026):

| Case | Union | Subtract | Intersect |
|---|---|---|---|
| Box & through cylinder | OK | OK | OK |
| Box & blind cylinder | OK | OK | OK |
| Crossing cylinders | OK | OK | OK |
| Box & shifted box (no coplanar faces) | OK | OK | OK |
| Box & sphere (any construction) | fails | fails | fails |
| Boxes sharing a coplanar face | fails | fails | fails |

Failures are reported on the feature with a clear message; the app does not crash on native
builds. Mitigations in place: coplanar join/cut from sketches retries with a 0.2 mm overlap.
Paths forward: track truck upstream; add an optional Open CASCADE backend (desktop only, LGPL)
behind the `calibro-kernel` interface; or contribute fixes upstream.

---

## 9. Roadmap and continuation point

**Stage 1 — Foundation (this session): done.** Workspace, kernel adapter, sketcher + solver,
feature history, native format, STEP/STL/OBJ/GLB, wgpu viewport, full UI shell, CI, docs.

**Stage 2 — next session (recommended order):**
1. Run CI on GitHub; fix anything the WebAssembly build reports; publish the web app.
2. Sketch: on-canvas dimension editing, trim/extend, curve–curve intersection splitting, splines.
3. Topological naming v1: sketches on faces store (body, face signature) and re-resolve.
4. ~~Chamfer (planar), Shell (planar), simple Loft (two profiles)~~ — done 2026-10-23 together with straight-edge fillet and line/arc sweep. Next: cylindrical shell, corner fillets.
5. 3MF export; IGES export study; STEP import as editable B-rep (convert stepio geometry).
6. Hide tangent seam edges; capped section view; supersampled image export.

**Stage 3+:** fillets (kernel strategy decision), sweep, patterns along paths, 2D drawings,
assemblies, materials with textures and HDR lighting, touch-first iPad layout.
