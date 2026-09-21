use super::*;

fn saved_session() -> Session {
    Session::new(Document::new(8, 6).unwrap(), Some("Saved.comp".into()))
}

fn rename(session: &mut Session, name: &str) {
    session
        .edit("Rename", |doc| {
            doc.layers[0].name = name.into();
            Ok(())
        })
        .unwrap();
}

#[test]
fn reverting_content_with_another_edit_does_not_restore_the_saved_revision() {
    let mut session = saved_session();
    let original = session.document.clone();
    rename(&mut session, "Changed");
    rename(&mut session, &original.layers[0].name);
    assert_eq!(session.document, original);
    assert!(session.dirty());
    session.undo();
    assert!(session.dirty());
    session.undo();
    assert!(!session.dirty());
    session.redo();
    session.redo();
    assert_eq!(session.document, original);
    assert!(session.dirty());
}

#[test]
fn a_branch_with_saved_content_is_modified_but_noops_and_failures_preserve_saved_state() {
    let mut session = saved_session();
    rename(&mut session, "Changed");
    session.mark_saved("Saved.comp".into());
    let saved = session.document.clone();
    session.undo();
    assert!(session.dirty());
    rename(&mut session, "Changed");
    assert_eq!(session.document, saved);
    assert!(session.dirty());
    assert!(session.redo_label().is_none());
    session.mark_saved("Saved.comp".into());
    session.edit("No-op", |_| Ok(())).unwrap();
    assert!(!session.dirty());
    assert!(
        session
            .edit("Invalid", |doc| {
                doc.width = 0;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(session.document, saved);
    assert!(!session.dirty());
    session.undo();
    session.edit("No-op", |_| Ok(())).unwrap();
    assert!(session.dirty());
    session.redo();
    assert!(!session.dirty());
}

#[test]
fn history_and_saving_beneath_a_preview_keep_the_committed_revision() {
    let mut session = saved_session();
    session.begin("Preview").unwrap();
    session.document.layers[0].name = "Preview".into();
    session
        .edit_committed("Rename", |doc| {
            doc.layers[0].name = "Committed".into();
            Ok(())
        })
        .unwrap();
    assert!(session.dirty());
    session.undo_committed();
    assert!(!session.dirty());
    session.redo_committed();
    assert!(session.dirty());
    session.mark_saved("Saved.comp".into());
    assert!(!session.dirty());
    session.commit().unwrap();
    assert!(session.dirty());
    session.undo();
    assert_eq!(session.document.layers[0].name, "Committed");
    assert!(!session.dirty());
    session.redo();
    assert_eq!(session.document.layers[0].name, "Preview");
    assert!(session.dirty());
    session.undo();
    session.begin("Cancelled").unwrap();
    session.document.layers[0].name = "Cancelled".into();
    session.cancel();
    assert!(!session.dirty());
    assert_eq!(session.redo_label(), Some("Preview"));
}

#[test]
fn pixel_selection_history_marks_modified_while_layer_navigation_does_not() {
    let mut session = saved_session();
    session
        .document
        .add(crate::document::Layer::blank("Other", 8, 6))
        .unwrap();
    session.mark_saved("Saved.comp".into());
    session.select_layer(session.document.layers[0].id, false);
    assert!(!session.dirty());
    session
        .edit("Select All", |doc| {
            doc.selection = Some(crate::selection::Selection::rectangle(
                8,
                6,
                [0., 0.],
                [8., 6.],
                false,
            ));
            Ok(())
        })
        .unwrap();
    assert!(session.dirty());
    session.undo();
    assert!(!session.dirty());
    session.redo();
    assert!(session.dirty());
}
