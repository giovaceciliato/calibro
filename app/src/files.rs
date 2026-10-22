//! Platform file access. Desktop uses native dialogs and writes files directly;
//! the browser picks files with the file chooser and saves by downloading.

use std::sync::mpsc::{channel, Receiver, Sender};

/// A file picked by the user (name + contents), delivered asynchronously.
pub struct PickedFile {
    pub name: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub path: Option<std::path::PathBuf>,
    pub bytes: Vec<u8>,
    pub purpose: Purpose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    OpenProject,
    Import,
}

pub struct Files {
    tx: Sender<PickedFile>,
    rx: Receiver<PickedFile>,
}

impl Default for Files {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx }
    }
}

impl Files {
    pub fn poll(&self) -> Option<PickedFile> {
        self.rx.try_recv().ok()
    }

    /// Opens a file chooser. The result arrives via [`Files::poll`].
    pub fn pick(&self, purpose: Purpose, filters: &[(&str, &[&str])]) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut dialog = rfd::FileDialog::new();
            for (name, exts) in filters {
                dialog = dialog.add_filter(*name, exts);
            }
            if let Some(path) = dialog.pick_file() {
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        let _ = self.tx.send(PickedFile {
                            name: path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            path: Some(path),
                            bytes,
                            purpose,
                        });
                    }
                    Err(e) => log::error!("cannot read {}: {e}", path.display()),
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let mut dialog = rfd::AsyncFileDialog::new();
            for (name, exts) in filters {
                dialog = dialog.add_filter(*name, exts);
            }
            let tx = self.tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(h) = dialog.pick_file().await {
                    let bytes = h.read().await;
                    let _ = tx.send(PickedFile {
                        name: h.file_name(),
                        path: None,
                        bytes,
                        purpose,
                    });
                }
            });
        }
    }
}

/// Desktop: blocking "save as" dialog. Returns the chosen path.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_dialog(default_name: &str, filter_name: &str, ext: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_file_name(default_name)
        .add_filter(filter_name, &[ext])
        .save_file()
        .map(|p| {
            if p.extension().is_none() {
                p.with_extension(ext)
            } else {
                p
            }
        })
}

/// Saves bytes under a user-chosen name. Desktop: dialog + write.
/// Browser: triggers a download. Returns a description of where it went.
pub fn save_bytes(
    default_name: &str,
    filter_name: &str,
    ext: &str,
    bytes: &[u8],
) -> Result<Option<String>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(path) = save_dialog(default_name, filter_name, ext) else {
            return Ok(None);
        };
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (filter_name, ext);
        download(default_name, bytes)?;
        Ok(Some(format!("downloaded {default_name}")))
    }
}

#[cfg(target_arch = "wasm32")]
pub fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    let err = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/octet-stream");
    let blob =
        web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &opts).map_err(err)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(err)?;
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let a: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(err)?
        .dyn_into()
        .map_err(|_| "anchor")?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}
