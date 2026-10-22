//! Editing session: owns the document, undo/redo history, unsaved-change
//! tracking and the evaluated model.

use crate::document::Document;
use crate::recompute::{Engine, Model};

const MAX_UNDO: usize = 200;

#[derive(Clone)]
struct Snapshot {
    doc: Document,
    revision: u64,
    label: String,
}

pub struct Editor {
    doc: Document,
    revision: u64,
    next_revision: u64,
    saved_revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    engine: Engine,
    model: Model,
    /// Label+key of the gesture currently being merged into one undo step.
    coalesce: Option<(String, u64)>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(Document::new())
    }
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let mut e = Self {
            doc,
            revision: 0,
            next_revision: 1,
            saved_revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            engine: Engine::new(),
            model: Model::default(),
            coalesce: None,
        };
        e.doc.fix_ids();
        e.recompute();
        e
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn recompute(&mut self) {
        self.model = self.engine.recompute(&self.doc);
    }

    /// Applies an undoable change and recomputes.
    pub fn apply<R>(&mut self, label: &str, f: impl FnOnce(&mut Document) -> R) -> R {
        let before = Snapshot {
            doc: self.doc.clone(),
            revision: self.revision,
            label: label.to_string(),
        };
        let r = f(&mut self.doc);
        if self.doc == before.doc {
            return r; // nothing changed: no history entry
        }
        self.undo.push(before);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision = self.next_revision;
        self.next_revision += 1;
        self.coalesce = None;
        self.recompute();
        r
    }

    /// Like [`Editor::apply`], but consecutive calls with the same `label` and
    /// `key` (e.g. while dragging a value or a point) become a single undo step
    /// until [`Editor::end_gesture`] is called.
    pub fn apply_coalesced<R>(
        &mut self,
        label: &str,
        key: u64,
        f: impl FnOnce(&mut Document) -> R,
    ) -> R {
        let same = self
            .coalesce
            .as_ref()
            .map(|(l, k)| l == label && *k == key)
            .unwrap_or(false);
        if !same || self.undo.is_empty() {
            let r = self.apply(label, f);
            if self.undo.last().map(|s| s.label == label).unwrap_or(false) {
                self.coalesce = Some((label.to_string(), key));
            }
            return r;
        }
        let before = self.doc.clone();
        let r = f(&mut self.doc);
        if self.doc != before {
            self.redo.clear();
            self.revision = self.next_revision;
            self.next_revision += 1;
            self.recompute();
        }
        r
    }

    /// Ends the current coalesced gesture (call on mouse release / Enter).
    pub fn end_gesture(&mut self) {
        self.coalesce = None;
    }

    /// Changes that should not create an undo step (e.g. camera moves) but
    /// still mark the project as modified.
    pub fn apply_silent(&mut self, f: impl FnOnce(&mut Document)) {
        f(&mut self.doc);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn undo(&mut self) -> bool {
        self.coalesce = None;
        let Some(s) = self.undo.pop() else {
            return false;
        };
        let camera = self.doc.camera.clone();
        self.redo.push(Snapshot {
            doc: std::mem::replace(&mut self.doc, s.doc),
            revision: self.revision,
            label: s.label,
        });
        self.doc.camera = camera; // undo never jumps the view
        self.revision = s.revision;
        self.recompute();
        true
    }

    pub fn redo(&mut self) -> bool {
        self.coalesce = None;
        let Some(s) = self.redo.pop() else {
            return false;
        };
        let camera = self.doc.camera.clone();
        self.undo.push(Snapshot {
            doc: std::mem::replace(&mut self.doc, s.doc),
            revision: self.revision,
            label: s.label,
        });
        self.doc.camera = camera;
        self.revision = s.revision;
        self.recompute();
        true
    }

    /// True when the document differs from the last saved/loaded state.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Replaces the whole document (new/open). Clears history.
    pub fn replace_document(&mut self, doc: Document) {
        *self = Editor::new(doc);
    }
}
