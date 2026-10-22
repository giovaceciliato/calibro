//! # calibro-io — exchange formats
//!
//! | Category | Format | Direction | Notes |
//! |---|---|---|---|
//! | CAD exchange (exact B-rep) | STEP AP242-style (`.step/.stp`) | export | exact solids; no history, constraints or materials |
//! | CAD exchange | STEP | import | tessellated into a reference mesh (not editable B-rep yet) |
//! | Mesh | STL binary/ASCII | export + import | triangles only, millimetres, Z-up |
//! | Mesh | OBJ + MTL | export + import | triangles + normals + colours, Y-up for Blender |
//! | Scene | glTF binary (`.glb`) | export | PBR materials, one node per body, metres, Y-up |
//!
//! None of these preserve Calibro's feature history; that is what `.calibro` is for.

use calibro_core::{BodyId, Document, EvaluatedBody, Material, MeshResource, Model};
use calibro_kernel::{BodyMesh, Solid};
use std::fmt::Write as _;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("nothing to export (no visible bodies)")]
    Empty,
    #[error("{0} cannot be exported to STEP because it is a reference mesh, not a solid")]
    NotSolid(String),
    #[error("the file could not be read: {0}")]
    Parse(String),
}

/// One body ready for export.
pub struct ExportBody<'a> {
    pub name: String,
    pub mesh: std::borrow::Cow<'a, BodyMesh>,
    pub material: Material,
    pub solid: Option<&'a Solid>,
}

/// Collects visible bodies (optionally only `selection`) from an evaluated model.
pub fn collect_bodies<'a>(
    doc: &Document,
    model: &'a Model,
    selection: Option<&[BodyId]>,
) -> Vec<ExportBody<'a>> {
    model
        .bodies
        .values()
        .filter(|b: &&EvaluatedBody| selection.map(|s| s.contains(&b.id)).unwrap_or(true))
        .filter(|b| doc.body_props(b.id).map(|p| p.visible).unwrap_or(true))
        .map(|b| {
            let props = doc.body_props(b.id);
            ExportBody {
                name: props
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| format!("Body {}", b.id)),
                mesh: std::borrow::Cow::Owned(weld_positions(&b.mesh, EXPORT_WELD_TOLERANCE)),
                material: props.map(|p| p.material.clone()).unwrap_or_default(),
                solid: b.solid(),
            }
        })
        .collect()
}

fn tri_iter<'a>(m: &'a BodyMesh) -> impl Iterator<Item = [[f32; 3]; 3]> + 'a {
    m.indices.chunks_exact(3).map(move |t| {
        [
            m.positions[t[0] as usize],
            m.positions[t[1] as usize],
            m.positions[t[2] as usize],
        ]
    })
}

/// Snaps vertices closer than `tol` to a single position so that faces of the
/// B-rep, which are tessellated separately, share exact boundary vertices.
/// This makes STL/OBJ output watertight for 3D printing.
pub fn weld_positions(m: &BodyMesh, tol: f32) -> BodyMesh {
    use std::collections::HashMap;
    let cell = |p: [f32; 3]| -> [i64; 3] { [0, 1, 2].map(|k| (p[k] / tol).floor() as i64) };
    let mut grid: HashMap<[i64; 3], Vec<[f32; 3]>> = HashMap::new();
    let mut out = m.clone();
    for p in out.positions.iter_mut() {
        let c = cell(*p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                        for q in list {
                            let d2 = (0..3).map(|k| (q[k] - p[k]).powi(2)).sum::<f32>();
                            if d2 <= tol * tol {
                                found = Some(*q);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        match found {
            Some(q) => *p = q,
            None => grid.entry(c).or_default().push(*p),
        }
    }
    // Drop triangles that collapsed.
    let pos = &out.positions;
    let idx: Vec<u32> = out
        .indices
        .chunks_exact(3)
        .filter(|t| {
            pos[t[0] as usize] != pos[t[1] as usize]
                && pos[t[1] as usize] != pos[t[2] as usize]
                && pos[t[0] as usize] != pos[t[2] as usize]
        })
        .flatten()
        .copied()
        .collect();
    out.indices = idx;
    out
}

/// Tolerance used when welding meshes for export (model units = mm).
pub const EXPORT_WELD_TOLERANCE: f32 = 1e-4;

fn tri_normal(t: &[[f32; 3]; 3]) -> [f32; 3] {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 {
        [n[0] / l, n[1] / l, n[2] / l]
    } else {
        [0.0, 0.0, 0.0]
    }
}

// --------------------------------------------------------------------- STL

pub fn export_stl_binary(bodies: &[ExportBody]) -> Result<Vec<u8>, IoError> {
    if bodies.is_empty() {
        return Err(IoError::Empty);
    }
    let count: usize = bodies.iter().map(|b| b.mesh.triangle_count()).sum();
    let mut out = Vec::with_capacity(84 + count * 50);
    let mut header = [0u8; 80];
    let h = b"Calibro STL export, units: millimetres";
    header[..h.len()].copy_from_slice(h);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for b in bodies {
        for t in tri_iter(&b.mesh) {
            for c in tri_normal(&t) {
                out.extend_from_slice(&c.to_le_bytes());
            }
            for p in t {
                for c in p {
                    out.extend_from_slice(&c.to_le_bytes());
                }
            }
            out.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    Ok(out)
}

pub fn export_stl_ascii(bodies: &[ExportBody]) -> Result<String, IoError> {
    if bodies.is_empty() {
        return Err(IoError::Empty);
    }
    let mut s = String::new();
    for b in bodies {
        let name: String = b
            .name
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .collect();
        let _ = writeln!(s, "solid {name}");
        for t in tri_iter(&b.mesh) {
            let n = tri_normal(&t);
            let _ = writeln!(s, "  facet normal {:e} {:e} {:e}", n[0], n[1], n[2]);
            s.push_str("    outer loop\n");
            for p in t {
                let _ = writeln!(s, "      vertex {:e} {:e} {:e}", p[0], p[1], p[2]);
            }
            s.push_str("    endloop\n  endfacet\n");
        }
        let _ = writeln!(s, "endsolid {name}");
    }
    Ok(s)
}

/// Reads binary or ASCII STL into a mesh resource (vertices welded).
pub fn import_stl(bytes: &[u8]) -> Result<MeshResource, IoError> {
    let mut tris: Vec<[[f32; 3]; 3]> = Vec::new();
    let is_binary = bytes.len() >= 84 && {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        84 + n * 50 == bytes.len()
    };
    if is_binary {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        for i in 0..n {
            let o = 84 + i * 50 + 12;
            tris.push([
                [f(o), f(o + 4), f(o + 8)],
                [f(o + 12), f(o + 16), f(o + 20)],
                [f(o + 24), f(o + 28), f(o + 32)],
            ]);
        }
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| IoError::Parse("not a valid STL file".into()))?;
        if !text.trim_start().starts_with("solid") {
            return Err(IoError::Parse("not a valid STL file".into()));
        }
        let mut cur: Vec<[f32; 3]> = Vec::new();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            if it.next() == Some("vertex") {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if v.len() != 3 {
                    return Err(IoError::Parse(format!("bad vertex line: {line}")));
                }
                cur.push([v[0], v[1], v[2]]);
                if cur.len() == 3 {
                    tris.push([cur[0], cur[1], cur[2]]);
                    cur.clear();
                }
            }
        }
    }
    if tris.is_empty() {
        return Err(IoError::Parse("the STL file contains no triangles".into()));
    }
    Ok(weld_triangles(&tris))
}

fn weld_triangles(tris: &[[[f32; 3]; 3]]) -> MeshResource {
    use std::collections::HashMap;
    let mut map: HashMap<[u32; 3], u32> = HashMap::new();
    let mut m = MeshResource::default();
    for t in tris {
        for p in t {
            let k = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
            let idx = *map.entry(k).or_insert_with(|| {
                m.positions.push(*p);
                (m.positions.len() - 1) as u32
            });
            m.indices.push(idx);
        }
    }
    m
}

// --------------------------------------------------------------------- OBJ

/// Calibro is Z-up; OBJ consumers (Blender, Maya, C4D) expect Y-up.
fn zup_to_yup(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}
fn yup_to_zup(p: [f32; 3]) -> [f32; 3] {
    [p[0], -p[2], p[1]]
}

/// Returns (obj, mtl). `mtl_file_name` is referenced by `mtllib`.
pub fn export_obj(bodies: &[ExportBody], mtl_file_name: &str) -> Result<(String, String), IoError> {
    if bodies.is_empty() {
        return Err(IoError::Empty);
    }
    let mut obj = String::new();
    let mut mtl = String::from("# Calibro material library\n");
    let _ = writeln!(
        obj,
        "# Calibro OBJ export, units: millimetres, Y-up\nmtllib {mtl_file_name}"
    );
    let mut base = 1usize;
    for (i, b) in bodies.iter().enumerate() {
        let mat = format!("mat{i}_{}", sanitize(&b.material.name));
        let c = b.material.color;
        let _ = writeln!(
            mtl,
            "newmtl {mat}\nKd {} {} {}\nKs 0.04 0.04 0.04\nNs {}\nd {}\nillum 2\n",
            c[0],
            c[1],
            c[2],
            ((1.0 - b.material.roughness) * 200.0).max(1.0),
            c[3]
        );
        let _ = writeln!(obj, "o {}", sanitize(&b.name));
        for p in &b.mesh.positions {
            let q = zup_to_yup(*p);
            let _ = writeln!(obj, "v {} {} {}", q[0], q[1], q[2]);
        }
        for n in &b.mesh.normals {
            let q = zup_to_yup(*n);
            let _ = writeln!(obj, "vn {} {} {}", q[0], q[1], q[2]);
        }
        let _ = writeln!(obj, "usemtl {mat}");
        for t in b.mesh.indices.chunks_exact(3) {
            let (a, bb, cc) = (
                t[0] as usize + base,
                t[1] as usize + base,
                t[2] as usize + base,
            );
            let _ = writeln!(obj, "f {a}//{a} {bb}//{bb} {cc}//{cc}");
        }
        base += b.mesh.positions.len();
    }
    Ok((obj, mtl))
}

fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if t.is_empty() {
        "unnamed".into()
    } else {
        t
    }
}

/// Imports OBJ geometry (positions + faces, polygons are fan-triangulated).
pub fn import_obj(text: &str) -> Result<MeshResource, IoError> {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut tris = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let v: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if v.len() != 3 {
                    return Err(IoError::Parse(format!("line {}: bad vertex", ln + 1)));
                }
                pos.push(yup_to_zup([v[0], v[1], v[2]]));
            }
            Some("f") => {
                let idx: Result<Vec<usize>, IoError> = it
                    .map(|tok| {
                        let first = tok.split('/').next().unwrap_or("");
                        let i: i64 = first
                            .parse()
                            .map_err(|_| IoError::Parse(format!("line {}: bad face", ln + 1)))?;
                        let i = if i < 0 { pos.len() as i64 + i } else { i - 1 };
                        if i < 0 || i as usize >= pos.len() {
                            return Err(IoError::Parse(format!(
                                "line {}: face index out of range",
                                ln + 1
                            )));
                        }
                        Ok(i as usize)
                    })
                    .collect();
                let idx = idx?;
                for k in 1..idx.len().saturating_sub(1) {
                    tris.push([pos[idx[0]], pos[idx[k]], pos[idx[k + 1]]]);
                }
            }
            _ => {}
        }
    }
    if tris.is_empty() {
        return Err(IoError::Parse("the OBJ file contains no faces".into()));
    }
    Ok(weld_triangles(&tris))
}

// --------------------------------------------------------------------- glTF (GLB)

/// Exports a self-contained binary glTF 2.0 scene. Units are metres and Y-up
/// (handled by a root node), so it imports at the right size in Blender etc.
pub fn export_glb(bodies: &[ExportBody]) -> Result<Vec<u8>, IoError> {
    use serde_json::json;
    if bodies.is_empty() {
        return Err(IoError::Empty);
    }
    let mut bin: Vec<u8> = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut materials = Vec::new();
    let mut nodes = vec![json!({
        "name": "Calibro model (mm, Z-up)",
        // mm -> m and Z-up -> Y-up (rotate -90 deg about X)
        "scale": [0.001, 0.001, 0.001],
        "rotation": [-std::f64::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f64::consts::FRAC_1_SQRT_2],
        "children": (1..=bodies.len()).collect::<Vec<_>>(),
    })];
    let pad = |bin: &mut Vec<u8>| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
    };
    for (i, b) in bodies.iter().enumerate() {
        let m = &*b.mesh;
        // positions
        pad(&mut bin);
        let off = bin.len();
        for p in &m.positions {
            for c in p {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        buffer_views.push(json!({"buffer": 0, "byteOffset": off, "byteLength": m.positions.len() * 12, "target": 34962}));
        accessors.push(json!({
            "bufferView": buffer_views.len() - 1, "componentType": 5126, "count": m.positions.len(), "type": "VEC3",
            "min": m.bbox_min, "max": m.bbox_max
        }));
        let pos_acc = accessors.len() - 1;
        // normals
        pad(&mut bin);
        let off = bin.len();
        for n in &m.normals {
            for c in n {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        buffer_views.push(json!({"buffer": 0, "byteOffset": off, "byteLength": m.normals.len() * 12, "target": 34962}));
        accessors.push(json!({"bufferView": buffer_views.len() - 1, "componentType": 5126, "count": m.normals.len(), "type": "VEC3"}));
        let nor_acc = accessors.len() - 1;
        // indices
        pad(&mut bin);
        let off = bin.len();
        for idx in &m.indices {
            bin.extend_from_slice(&idx.to_le_bytes());
        }
        buffer_views.push(json!({"buffer": 0, "byteOffset": off, "byteLength": m.indices.len() * 4, "target": 34963}));
        accessors.push(json!({"bufferView": buffer_views.len() - 1, "componentType": 5125, "count": m.indices.len(), "type": "SCALAR"}));
        let idx_acc = accessors.len() - 1;

        // glTF wants linear colour factors; Calibro stores sRGB.
        let m8 = b.material.color;
        let c = [
            srgb_to_linear(m8[0]),
            srgb_to_linear(m8[1]),
            srgb_to_linear(m8[2]),
            m8[3],
        ];
        let mut mat = json!({
            "name": b.material.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": c,
                "metallicFactor": b.material.metallic,
                "roughnessFactor": b.material.roughness,
            },
            "doubleSided": false,
        });
        if c[3] < 1.0 {
            mat["alphaMode"] = json!("BLEND");
        }
        materials.push(mat);
        meshes.push(json!({
            "name": b.name,
            "primitives": [{"attributes": {"POSITION": pos_acc, "NORMAL": nor_acc}, "indices": idx_acc, "material": i, "mode": 4}]
        }));
        nodes.push(json!({"name": b.name, "mesh": i}));
    }
    pad(&mut bin);
    let doc = json!({
        "asset": {"version": "2.0", "generator": format!("Calibro {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [0]}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": bin.len()}],
    });
    let mut js = serde_json::to_vec(&doc).map_err(|e| IoError::Parse(e.to_string()))?;
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// --------------------------------------------------------------------- STEP

/// Exports exact B-rep solids as a STEP file (truck-stepio, ISO 10303-42 entities).
pub fn export_step(bodies: &[ExportBody], file_name: &str) -> Result<String, IoError> {
    use truck_stepio::out::{CompleteStepDisplay, StepHeaderDescriptor, StepModels};
    if bodies.is_empty() {
        return Err(IoError::Empty);
    }
    let mut compressed = Vec::new();
    for b in bodies {
        let s = b.solid.ok_or_else(|| IoError::NotSolid(b.name.clone()))?;
        compressed.push(s.compress());
    }
    let models: StepModels<_, _, _> = compressed.iter().collect();
    let text = CompleteStepDisplay::new(
        models,
        StepHeaderDescriptor {
            file_name: file_name.to_string(),
            organization_system: format!("Calibro {}", env!("CARGO_PKG_VERSION")),
            ..Default::default()
        },
    )
    .to_string();
    Ok(text)
}

/// Imports a STEP file as a tessellated reference mesh.
pub fn import_step(text: &str) -> Result<MeshResource, IoError> {
    use truck_meshalgo::prelude::*;
    use truck_stepio::r#in::Table;
    let table = Table::from_step(text)
        .ok_or_else(|| IoError::Parse("could not parse the STEP file".into()))?;
    let mut tris = Vec::new();
    let mut shells = 0;
    let mut failed = 0;
    for shell in table.shell.values() {
        let Ok(shell) = table.to_compressed_shell(shell) else {
            failed += 1;
            continue;
        };
        shells += 1;
        let pre = shell.robust_triangulation(0.05).to_polygon();
        let bdd = pre.bounding_box();
        let tol = (bdd.diameter() * 0.001).max(1e-4);
        let poly = shell.robust_triangulation(tol).to_polygon();
        let p = poly.positions();
        for t in poly.faces().triangle_iter() {
            let g = |i: usize| {
                let q = p[t[i].pos];
                [q.x as f32, q.y as f32, q.z as f32]
            };
            tris.push([g(0), g(1), g(2)]);
        }
    }
    if tris.is_empty() {
        return Err(IoError::Parse(format!(
            "no supported solid geometry found ({shells} shells read, {failed} unsupported)"
        )));
    }
    Ok(weld_triangles(&tris))
}

#[cfg(test)]
mod tests {
    use super::*;
    use calibro_core::*;

    fn model() -> (Document, Editor) {
        let mut doc = Document::new();
        doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Box {
                    min: [0.0; 3],
                    size: [10.0, 20.0, 30.0],
                },
                body: BodyMode::NewBody,
            },
        );
        doc.add_feature(
            "",
            FeatureKind::Primitive {
                primitive: Primitive::Cylinder {
                    base: [40.0, 0.0, 0.0],
                    radius: 5.0,
                    height: 10.0,
                },
                body: BodyMode::NewBody,
            },
        );
        let ed = Editor::new(doc.clone());
        (doc, ed)
    }

    fn mesh_volume(m: &MeshResource) -> f64 {
        let mut v = 0.0;
        for t in m.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| m.positions[t[k] as usize].map(|x| x as f64));
            v += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
        }
        v
    }

    #[test]
    fn stl_binary_and_ascii_round_trip() {
        let (doc, ed) = model();
        let bodies = collect_bodies(&doc, ed.model(), None);
        let expected: f64 = bodies.iter().map(|b| b.mesh.volume()).sum();
        let bin = export_stl_binary(&bodies).unwrap();
        let m = import_stl(&bin).unwrap();
        assert!(
            (mesh_volume(&m) - expected).abs() < 1e-2,
            "{}",
            mesh_volume(&m)
        );
        let ascii = export_stl_ascii(&bodies).unwrap();
        let m2 = import_stl(ascii.as_bytes()).unwrap();
        assert!((mesh_volume(&m2) - expected).abs() / expected < 1e-5);
        assert!(import_stl(b"garbage").is_err());
    }

    #[test]
    fn obj_round_trip() {
        let (doc, ed) = model();
        let bodies = collect_bodies(&doc, ed.model(), None);
        let expected: f64 = bodies.iter().map(|b| b.mesh.volume()).sum();
        let (obj, mtl) = export_obj(&bodies, "model.mtl").unwrap();
        assert!(mtl.contains("newmtl") && obj.contains("mtllib model.mtl"));
        let m = import_obj(&obj).unwrap();
        assert!((mesh_volume(&m) - expected).abs() / expected < 1e-5);
    }

    #[test]
    fn glb_structure() {
        let (doc, ed) = model();
        let bodies = collect_bodies(&doc, ed.model(), None);
        let glb = export_glb(&bodies).unwrap();
        assert_eq!(&glb[0..4], b"glTF");
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
        let jlen = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let js: serde_json::Value = serde_json::from_slice(&glb[20..20 + jlen]).unwrap();
        assert_eq!(js["meshes"].as_array().unwrap().len(), 2);
        assert_eq!(js["nodes"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn step_export_then_import() {
        let (doc, ed) = model();
        let bodies = collect_bodies(&doc, ed.model(), None);
        let expected: f64 = bodies.iter().map(|b| b.mesh.volume()).sum();
        let step = export_step(&bodies, "test.step").unwrap();
        assert!(step.starts_with("ISO-10303-21;"));
        assert!(step.contains("MANIFOLD_SOLID_BREP"));
        let m = import_step(&step).unwrap();
        let v = mesh_volume(&m);
        assert!(
            (v - expected).abs() / expected < 1e-2,
            "re-imported volume {v} vs {expected}"
        );
    }

    #[test]
    fn step_refuses_reference_mesh() {
        let mut doc = Document::new();
        doc.resources.insert(
            "m".into(),
            MeshResource {
                positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
            }
            .to_bytes(),
        );
        doc.add_feature(
            "",
            FeatureKind::ImportedMesh {
                resource: "m".into(),
            },
        );
        let ed = Editor::new(doc.clone());
        let bodies = collect_bodies(&doc, ed.model(), None);
        assert!(matches!(
            export_step(&bodies, "x"),
            Err(IoError::NotSolid(_))
        ));
    }
}
