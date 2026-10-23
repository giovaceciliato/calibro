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
    coalesce_redo: Option<Vec<Snapshot>>,
    coalesce_evicted: Option<Snapshot>,
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
            coalesce_redo: None,
            coalesce_evicted: None,
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
        self.end_gesture();
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
            let revision = self.revision;
            let redo = self.redo.clone();
            let evicted = if self.undo.len() == MAX_UNDO {
                self.undo.first().cloned()
            } else {
                None
            };
            let r = self.apply(label, f);
            if self.revision != revision {
                self.coalesce = Some((label.to_string(), key));
                self.coalesce_redo = Some(redo);
                self.coalesce_evicted = evicted;
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

    /// Evaluate a candidate using the existing cache, and commit it only if accepted.
    /// Rejected previews do not change geometry, undo/redo, or the dirty flag.
    pub fn apply_coalesced_checked(
        &mut self,
        label: &str,
        key: u64,
        edit: impl FnOnce(&mut Document),
        validate: impl FnOnce(&Model) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut candidate = self.doc.clone();
        edit(&mut candidate);
        if candidate == self.doc {
            return Ok(());
        }
        let preview = self.engine.recompute(&candidate);
        validate(&preview)?;
        // The accepted candidate is now cached, so this does not redo kernel operations.
        self.apply_coalesced(label, key, |doc| *doc = candidate);
        Ok(())
    }

    /// Ends the current coalesced gesture (call on mouse release / Enter).
    pub fn end_gesture(&mut self) {
        self.coalesce = None;
        self.coalesce_redo = None;
        self.coalesce_evicted = None;
    }

    /// Cancel an active gesture, restoring its document, dirty state and redo history.
    pub fn cancel_gesture(&mut self) -> bool {
        if self.coalesce.take().is_none() {
            return false;
        }
        let Some(before) = self.undo.pop() else {
            return false;
        };
        let camera = self.doc.camera.clone();
        self.doc = before.doc;
        self.doc.camera = camera;
        self.revision = before.revision;
        self.redo = self.coalesce_redo.take().unwrap_or_default();
        if let Some(evicted) = self.coalesce_evicted.take() {
            self.undo.insert(0, evicted);
        }
        self.recompute();
        true
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
        self.end_gesture();
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
        self.end_gesture();
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

#[cfg(test)]
mod gesture_tests {
    use super::*;

    fn title(ed: &mut Editor, label: &str, value: &str) {
        ed.apply(label, |doc| doc.metadata.title = value.into());
    }

    #[test]
    fn rejected_preview_preserves_document_dirty_state_and_redo() {
        let mut ed = Editor::default();
        title(&mut ed, "one", "one");
        title(&mut ed, "two", "two");
        ed.undo();
        ed.mark_saved();
        let before = ed.doc().clone();
        let result = ed.apply_coalesced_checked(
            "drag",
            1,
            |doc| doc.metadata.title = "invalid".into(),
            |_| Err("limit".into()),
        );
        assert!(result.is_err());
        assert_eq!(ed.doc(), &before);
        assert!(!ed.is_dirty());
        assert_eq!(ed.redo_label(), Some("two"));
        assert_eq!(ed.undo_label(), Some("one"));
    }

    #[test]
    fn cancel_gesture_restores_saved_state_and_previous_redo() {
        let mut ed = Editor::default();
        title(&mut ed, "one", "one");
        title(&mut ed, "two", "two");
        ed.undo();
        ed.mark_saved();
        for value in ["preview-a", "preview-b"] {
            ed.apply_coalesced_checked(
                "drag",
                1,
                |doc| doc.metadata.title = value.into(),
                |_| Ok(()),
            )
            .unwrap();
        }
        assert!(ed.is_dirty());
        assert!(ed.cancel_gesture());
        assert_eq!(ed.doc().metadata.title, "one");
        assert!(!ed.is_dirty());
        assert_eq!(ed.undo_label(), Some("one"));
        assert!(ed.redo());
        assert_eq!(ed.doc().metadata.title, "two");
    }

    #[test]
    fn accepted_previews_are_one_undo_and_redo_step() {
        let mut ed = Editor::default();
        title(&mut ed, "initial", "initial");
        for value in ["preview-a", "preview-b", "final"] {
            ed.apply_coalesced_checked(
                "drag",
                1,
                |doc| doc.metadata.title = value.into(),
                |_| Ok(()),
            )
            .unwrap();
        }
        ed.end_gesture();
        assert!(!ed.cancel_gesture());
        ed.undo();
        assert_eq!(ed.doc().metadata.title, "initial");
        ed.redo();
        assert_eq!(ed.doc().metadata.title, "final");
    }

    #[test]
    fn no_op_does_not_cancel_an_earlier_edit_with_the_same_label() {
        let mut ed = Editor::default();
        title(&mut ed, "drag", "earlier");
        ed.apply_coalesced("drag", 1, |_| {});
        assert!(!ed.cancel_gesture());
        assert_eq!(ed.doc().metadata.title, "earlier");
        assert!(ed.undo());
    }

    #[test]
    fn cancel_at_history_limit_does_not_evict_a_previous_undo_step() {
        let mut ed = Editor::default();
        for i in 0..MAX_UNDO {
            title(&mut ed, "edit", &format!("{i}"));
        }
        ed.apply_coalesced("drag", 1, |doc| doc.metadata.title = "preview".into());
        assert!(ed.cancel_gesture());
        let mut count = 0;
        while ed.undo() {
            count += 1;
        }
        assert_eq!(count, MAX_UNDO);
    }
}
