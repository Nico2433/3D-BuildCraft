use buildcraft3d::history::{History, Snapshot};
use buildcraft3d::primitives;
use buildcraft3d::scene::Object;

fn snap(n: usize) -> Snapshot {
    Snapshot {
        objects: (0..n)
            .map(|_| Object::new("c", primitives::cube(1.0), [0, 0, 0, 255]))
            .collect(),
        selection: Default::default(),
    }
}

#[test]
fn undo_and_redo_walk_back_and_forth_through_the_states() {
    let mut h = History::default();
    assert!(!h.can_undo() && !h.can_redo());
    assert!(h.undo(snap(9)).is_none());
    h.push(snap(1));
    h.push(snap(2));
    let two = h.undo(snap(3)).unwrap();
    assert_eq!(two.objects.len(), 2);
    let one = h.undo(snap(2)).unwrap();
    assert_eq!(one.objects.len(), 1);
    assert!(!h.can_undo());
    assert_eq!(h.redo(snap(1)).unwrap().objects.len(), 2);
    assert_eq!(h.redo(snap(2)).unwrap().objects.len(), 3);
    assert!(!h.can_redo());
}

#[test]
fn a_new_edit_discards_the_redo_stack() {
    let mut h = History::default();
    h.push(snap(1));
    h.undo(snap(2)).unwrap();
    assert!(h.can_redo());
    h.push(snap(1));
    assert!(!h.can_redo());
}

#[test]
fn history_keeps_only_the_last_hundred_states() {
    let mut h = History::default();
    for i in 0..150 {
        h.push(snap(i));
    }
    let mut n = 0;
    let mut oldest = 0;
    while let Some(s) = h.undo(snap(0)) {
        n += 1;
        oldest = s.objects.len();
    }
    assert_eq!(n, 100);
    assert_eq!(oldest, 50, "the first 50 states were dropped");
}

#[test]
fn clear_empties_both_stacks() {
    let mut h = History::default();
    h.push(snap(1));
    h.undo(snap(2));
    h.clear();
    assert!(!h.can_undo() && !h.can_redo());
}
