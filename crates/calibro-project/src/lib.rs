//! # calibro-project — the native `.calibro` project format
//!
//! A `.calibro` file is a ZIP container (the same approach used by `.docx`,
//! `.3mf` and many CAD formats):
//!
//! ```text
//! mimetype            "application/vnd.calibro.project" (stored, first entry)
//! manifest.json       format name, format version, app version, entry list
//! document.json       the full editable Document (feature history, sketches,
//!                     constraints, parameters, materials, camera, metadata)
//! resources/<name>    binary resources (imported meshes, exact B-rep solids)
//! ```
//!
//! Geometry is *not* stored as the source of truth: it is recomputed from the
//! feature history when the file opens, so the project stays fully editable.
//!
//! Versioning: `format_version` is an integer. Files newer than
//! [`FORMAT_VERSION`] are refused with [`ProjectError::UnsupportedVersion`];
//! older versions pass through [`migrate`].

use calibro_core::Document;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Seek, Write};

pub const EXTENSION: &str = "calibro";
pub const MIME_TYPE: &str = "application/vnd.calibro.project";
pub const FORMAT_NAME: &str = "calibro-project";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("this is not a Calibro project file ({0})")]
    NotAProject(String),
    #[error("the project file is damaged or incomplete: {0}")]
    Corrupt(String),
    #[error("this project was saved by a newer version of Calibro (format {found}, this app supports up to {supported}). Please update Calibro.")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub app_version: String,
    pub document: String,
    #[serde(default)]
    pub resources: Vec<String>,
}

fn zip_err(e: zip::result::ZipError) -> ProjectError {
    ProjectError::Corrupt(e.to_string())
}

/// Serializes a document into `.calibro` bytes.
pub fn save_to_bytes(doc: &Document) -> Result<Vec<u8>, ProjectError> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("mimetype", stored).map_err(zip_err)?;
        zip.write_all(MIME_TYPE.as_bytes())?;

        let resources: Vec<String> = doc
            .resources
            .keys()
            .map(|k| format!("resources/{k}"))
            .collect();
        let manifest = Manifest {
            format: FORMAT_NAME.into(),
            format_version: FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
            document: "document.json".into(),
            resources: resources.clone(),
        };
        zip.start_file("manifest.json", deflated).map_err(zip_err)?;
        zip.write_all(
            &serde_json::to_vec_pretty(&manifest)
                .map_err(|e| ProjectError::Corrupt(e.to_string()))?,
        )?;

        zip.start_file("document.json", deflated).map_err(zip_err)?;
        zip.write_all(
            &serde_json::to_vec_pretty(doc).map_err(|e| ProjectError::Corrupt(e.to_string()))?,
        )?;

        for (name, data) in &doc.resources {
            zip.start_file(format!("resources/{name}"), deflated)
                .map_err(zip_err)?;
            zip.write_all(data)?;
        }
        zip.finish().map_err(zip_err)?;
    }
    Ok(buf.into_inner())
}

fn read_entry<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, ProjectError> {
    let mut f = zip
        .by_name(name)
        .map_err(|_| ProjectError::Corrupt(format!("missing '{name}'")))?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)
        .map_err(|e| ProjectError::Corrupt(format!("cannot read '{name}': {e}")))?;
    Ok(v)
}

/// Upgrades older document JSON to the current format version.
pub fn migrate(
    value: serde_json::Value,
    from_version: u32,
) -> Result<serde_json::Value, ProjectError> {
    match from_version {
        FORMAT_VERSION => Ok(value),
        // Future: 1 => migrate_1_to_2(value) ...
        v => Err(ProjectError::UnsupportedVersion {
            found: v,
            supported: FORMAT_VERSION,
        }),
    }
}

/// Parses `.calibro` bytes into a document. Validates container, manifest,
/// version and every referenced entry.
pub fn load_from_bytes(bytes: &[u8]) -> Result<Document, ProjectError> {
    if bytes.len() < 4 || &bytes[0..2] != b"PK" {
        return Err(ProjectError::NotAProject("not a ZIP container".into()));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?;
    let mime = read_entry(&mut zip, "mimetype")
        .map_err(|_| ProjectError::NotAProject("no mimetype entry".into()))?;
    if mime != MIME_TYPE.as_bytes() {
        return Err(ProjectError::NotAProject(format!(
            "unexpected type '{}'",
            String::from_utf8_lossy(&mime)
        )));
    }
    let manifest: Manifest = serde_json::from_slice(&read_entry(&mut zip, "manifest.json")?)
        .map_err(|e| ProjectError::Corrupt(format!("manifest.json: {e}")))?;
    if manifest.format != FORMAT_NAME {
        return Err(ProjectError::NotAProject(format!(
            "format '{}'",
            manifest.format
        )));
    }
    if manifest.format_version > FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion {
            found: manifest.format_version,
            supported: FORMAT_VERSION,
        });
    }
    let raw: serde_json::Value = serde_json::from_slice(&read_entry(&mut zip, &manifest.document)?)
        .map_err(|e| ProjectError::Corrupt(format!("{}: {e}", manifest.document)))?;
    let value = migrate(raw, manifest.format_version)?;
    let mut doc: Document = serde_json::from_value(value)
        .map_err(|e| ProjectError::Corrupt(format!("{}: {e}", manifest.document)))?;
    for entry in &manifest.resources {
        let data = read_entry(&mut zip, entry)?;
        let name = entry
            .strip_prefix("resources/")
            .unwrap_or(entry)
            .to_string();
        doc.resources.insert(name, data);
    }
    // Validate sketches reference existing entities.
    for f in &doc.features {
        if let calibro_core::FeatureKind::Sketch { sketch, .. } = &f.kind {
            sketch
                .validate()
                .map_err(|e| ProjectError::Corrupt(format!("sketch '{}': {e}", f.name)))?;
        }
    }
    doc.fix_ids();
    Ok(doc)
}

/// Saves to disk safely: writes a temporary file, keeps a `.bak` of the
/// previous version, then atomically renames. A failed save never destroys
/// the existing file.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_to_path(doc: &Document, path: &std::path::Path) -> Result<(), ProjectError> {
    let bytes = save_to_bytes(doc)?;
    // Verify before touching the destination.
    load_from_bytes(&bytes)?;
    let tmp = path.with_extension("calibro.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    if path.exists() {
        let bak = path.with_extension("calibro.bak");
        let _ = std::fs::copy(path, bak);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_from_path(path: &std::path::Path) -> Result<Document, ProjectError> {
    load_from_bytes(&std::fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calibro_core::kernel::Plane;
    use calibro_core::sketch::{Constraint, Sketch};
    use calibro_core::*;

    fn sample() -> Document {
        let mut doc = Document::new();
        doc.metadata.title = "Bracket".into();
        let mut s = Sketch::new();
        let [b, r, _, _] = s.add_rectangle([0.0, 0.0], [30.0, 20.0]);
        s.add_circle([15.0, 10.0], 4.0);
        s.add_constraint(Constraint::Length {
            line: b,
            value: 30.0,
        });
        s.add_constraint(Constraint::Length {
            line: r,
            value: 20.0,
        });
        let sk = doc.add_feature(
            "Base sketch",
            FeatureKind::Sketch {
                plane: Plane::XY,
                sketch: s,
                on_body: None,
            },
        );
        let ex = doc.add_feature(
            "Plate",
            FeatureKind::Extrude {
                sketch: sk,
                profiles: ProfileSelection::Seeds {
                    points: vec![[2.0, 2.0]],
                },
                distance: 6.0,
                symmetric: false,
                body: BodyMode::NewBody,
            },
        );
        let body = BodyId {
            feature: ex,
            index: 0,
        };
        let p = doc.body_props_mut(body);
        p.name = "Plate body".into();
        p.material = material_presets()[3].clone();
        doc.camera.yaw = 1.23;
        doc.units = Units::Inches;
        doc.resources.insert(
            "mesh0.bin".into(),
            MeshResource {
                positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
            }
            .to_bytes(),
        );
        doc.add_feature(
            "Ref",
            FeatureKind::ImportedMesh {
                resource: "mesh0.bin".into(),
            },
        );
        doc
    }

    #[test]
    fn round_trip_preserves_editable_state() {
        let doc = sample();
        let bytes = save_to_bytes(&doc).unwrap();
        let back = load_from_bytes(&bytes).unwrap();
        assert_eq!(back, doc, "document must survive save/load exactly");
        // Geometry is recomputed identically from the history.
        let a = Editor::new(doc);
        let b = Editor::new(back.clone());
        let ka: Vec<_> = a.model().bodies.keys().copied().collect();
        let kb: Vec<_> = b.model().bodies.keys().copied().collect();
        assert_eq!(ka, kb);
        for k in ka {
            let (va, vb) = (
                a.model().bodies[&k].mesh.volume(),
                b.model().bodies[&k].mesh.volume(),
            );
            assert!((va - vb).abs() < 1e-9);
        }
        // and stays editable: change the extrude distance after loading
        let mut ed = Editor::new(back);
        let ex = ed.doc().features[1].id;
        ed.apply("edit", |d| {
            if let FeatureKind::Extrude { distance, .. } = &mut d.feature_mut(ex).unwrap().kind {
                *distance = 12.0;
            }
        });
        let v = ed.model().bodies[&BodyId {
            feature: ex,
            index: 0,
        }]
            .mesh
            .volume();
        let exp = (600.0 - std::f64::consts::PI * 16.0) * 12.0;
        assert!((v - exp).abs() / exp < 5e-3, "{v} vs {exp}");
    }

    #[test]
    fn rejects_garbage_and_wrong_zip() {
        assert!(matches!(
            load_from_bytes(b"hello"),
            Err(ProjectError::NotAProject(_))
        ));
        // a zip without our mimetype
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file("readme.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b"hi").unwrap();
            z.finish().unwrap();
        }
        assert!(matches!(
            load_from_bytes(buf.get_ref()),
            Err(ProjectError::NotAProject(_))
        ));
    }

    #[test]
    fn truncated_file_is_reported_as_corrupt() {
        let bytes = save_to_bytes(&sample()).unwrap();
        let cut = &bytes[..bytes.len() / 2];
        let err = load_from_bytes(cut).unwrap_err();
        assert!(
            matches!(err, ProjectError::Corrupt(_) | ProjectError::NotAProject(_)),
            "{err}"
        );
    }

    #[test]
    fn newer_version_is_refused() {
        let doc = sample();
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("mimetype", o).unwrap();
            z.write_all(MIME_TYPE.as_bytes()).unwrap();
            z.start_file("manifest.json", o).unwrap();
            let m = Manifest {
                format: FORMAT_NAME.into(),
                format_version: FORMAT_VERSION + 1,
                app_version: "9.9.9".into(),
                document: "document.json".into(),
                resources: vec![],
            };
            z.write_all(&serde_json::to_vec(&m).unwrap()).unwrap();
            z.start_file("document.json", o).unwrap();
            z.write_all(&serde_json::to_vec(&doc).unwrap()).unwrap();
            z.finish().unwrap();
        }
        assert!(matches!(
            load_from_bytes(buf.get_ref()),
            Err(ProjectError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn missing_resource_is_corrupt() {
        let mut doc = sample();
        let bytes = {
            // write manifest that lists a resource but omit the entry
            doc.resources.clear();
            let mut b = save_to_bytes(&doc).unwrap();
            // patch: re-create with a manifest pointing to a missing resource
            let mut buf = Cursor::new(Vec::new());
            {
                let mut z = zip::ZipWriter::new(&mut buf);
                let o = zip::write::SimpleFileOptions::default();
                z.start_file("mimetype", o).unwrap();
                z.write_all(MIME_TYPE.as_bytes()).unwrap();
                z.start_file("manifest.json", o).unwrap();
                let m = Manifest {
                    format: FORMAT_NAME.into(),
                    format_version: FORMAT_VERSION,
                    app_version: "0".into(),
                    document: "document.json".into(),
                    resources: vec!["resources/mesh0.bin".into()],
                };
                z.write_all(&serde_json::to_vec(&m).unwrap()).unwrap();
                z.start_file("document.json", o).unwrap();
                z.write_all(&serde_json::to_vec(&doc).unwrap()).unwrap();
                z.finish().unwrap();
            }
            b.clear();
            buf.into_inner()
        };
        assert!(matches!(
            load_from_bytes(&bytes),
            Err(ProjectError::Corrupt(_))
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn save_to_path_keeps_backup() {
        let dir = std::env::temp_dir().join(format!("calibro-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("part.calibro");
        let mut doc = sample();
        save_to_path(&doc, &path).unwrap();
        doc.metadata.title = "Second".into();
        save_to_path(&doc, &path).unwrap();
        assert_eq!(load_from_path(&path).unwrap().metadata.title, "Second");
        assert_eq!(
            load_from_path(&path.with_extension("calibro.bak"))
                .unwrap()
                .metadata
                .title,
            "Bracket"
        );
        assert!(!path.with_extension("calibro.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
