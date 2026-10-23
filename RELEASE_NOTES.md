# Calibro 0.1.0 — first public preview

The first release of Calibro, an open-source precision 3D CAD application written in Rust.
**Try it in your browser:** https://giovaceciliato.github.io/calibro/

## Highlights
- Sketch on planes or flat faces: lines, rectangles, circles, arcs, polygons, construction geometry
- Constraints and dimensions with a real solver (fully / under / over-defined feedback)
- Extrude (new body / join / cut / intersect, drag handle), revolve, box, cylinder, sphere
- Booleans, move/rotate/scale, linear & circular patterns, mirror
- Parametric history: edit any step and everything after it rebuilds; undo/redo
- Materials, X-ray, wireframe, section view, measuring
- Native editable `.calibro` projects; export STEP, GLB, OBJ, STL; import STEP/STL/OBJ as reference

## Downloads
| System | File |
|---|---|
| Windows 10/11 (64-bit) | `calibro-windows-x86_64.zip` → unzip → run `calibro.exe` |
| macOS (Apple Silicon) | `calibro-macos-arm64.tar.gz` |
| Fedora 40+ (x86_64) | `calibro-0.1.0-1.x86_64.rpm` → double-click, or `sudo dnf install ./calibro-0.1.0-1.x86_64.rpm` |
| Any Linux, incl. Bazzite/Silverblue (x86_64) | `calibro-0.1.0-linux-x86_64.tar.gz` → extract → `./install.sh` (installs for your user, adds a menu entry) |
| Linux, bare binary | `calibro-linux-x86_64.tar.gz` |

The apps are not code-signed yet:
- **Windows:** SmartScreen may warn → *More info* → *Run anyway*.
- **macOS:** after extracting, open Terminal in that folder and run
  `xattr -d com.apple.quarantine calibro && ./calibro`
- **Linux:** needs a recent system (glibc 2.39+: Fedora 40+, Ubuntu 24.04+) and Vulkan or
  OpenGL drivers (Fedora: `mesa-vulkan-drivers`, installed by default).

## Known limitations
Fillet, chamfer, shell, sweep and loft are not available yet. Booleans with spheres and
with faces lying exactly on top of each other fail (the failing step is shown in red in the
history). Full status: [docs/FEATURE_TRACKER.md](docs/FEATURE_TRACKER.md).
