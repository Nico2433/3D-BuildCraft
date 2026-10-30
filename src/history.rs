//! Undo/redo by scene snapshots. Meshes are shared through `Arc`, so a snapshot only
//! copies object headers, not geometry.

use crate::scene::Object;
use std::collections::BTreeSet;

const LIMIT: usize = 100;

#[derive(Clone)]
pub struct Snapshot {
    pub objects: Vec<Object>,
    pub selection: BTreeSet<u64>,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl History {
    /// Call with the state *before* an edit.
    pub fn push(&mut self, before: Snapshot) {
        self.undo.push(before);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// How many steps can be undone.
    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    /// Forgets the steps after the first `depth` ones (what a cancelled tool did).
    pub fn truncate(&mut self, depth: usize) {
        self.undo.truncate(depth);
        self.redo.clear();
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(n: usize) -> Snapshot {
        Snapshot {
            objects: Vec::with_capacity(n),
            selection: (0..n as u64).collect(),
        }
    }

    #[test]
    fn undo_then_redo_restores_states() {
        let mut h = History::default();
        h.push(snap(1));
        let back = h.undo(snap(2)).unwrap();
        assert_eq!(back.selection.len(), 1);
        let fwd = h.redo(back).unwrap();
        assert_eq!(fwd.selection.len(), 2);
        assert!(!h.can_redo());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::default();
        h.push(snap(1));
        h.undo(snap(2));
        h.push(snap(3));
        assert!(!h.can_redo());
    }
}
