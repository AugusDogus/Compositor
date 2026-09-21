use super::*;

#[test]
fn window_deactivation_dismisses_menus_without_invoking_or_reopening_them() {
    for keys in [Some("alt-f"), Some("alt-l right"), None] {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(8, 8).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [80, 120, 160, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = quickgui::Application::new()
            .bind_keys(quickgui::menubar_key_bindings())
            .bind_keys(quickgui::popover_menu_key_bindings())
            .bind_keys(key_bindings())
            .into_test_context(
                quickgui::WindowOptions::new("Menu deactivation").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        if let Some(keys) = keys {
            cx.simulate_keystrokes(window, keys).unwrap();
        } else {
            cx.click(window, "layer-adjustment-menu").unwrap();
        }
        assert!(cx.read(view, |e| e.menus.is_open()).unwrap());
        if keys == Some("alt-l right") {
            assert!(cx.read(view, |e| e.menus.submenu_anchor.is_some()).unwrap());
        }
        cx.update(view, |e, cx| e.event(&Event::Focused(false), cx))
            .unwrap();
        cx.read(view, |e| {
            assert!(!e.menus.is_open());
            assert!(e.menus.submenu_anchor.is_none());
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        cx.update(view, |e, cx| e.event(&Event::Focused(true), cx))
            .unwrap();
        assert!(!cx.read(view, |e| e.menus.is_open()).unwrap());
        for hidden_menu in ["application-menu-items", "application-submenu-items"] {
            assert_ne!(
                cx.focused(window).unwrap(),
                Some(quickgui::ElementId::from(hidden_menu))
            );
        }
        // A new menu session can still navigate and invoke its command.
        cx.simulate_keystrokes(window, "alt-i").unwrap();
        assert_eq!(cx.read(view, |e| e.menus.bar.open_menu()).unwrap(), Some(4));
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(quickgui::ElementId::from("application-menu-items")),
            "Menu reopened after {keys:?}"
        );
        cx.simulate_keystrokes(window, "home enter").unwrap();
        assert_eq!(
            cx.read(view, |e| e.adjustment_edit.as_ref().unwrap().settings.kind)
                .unwrap(),
            Kind::Curves
        );
        cx.click(window, "form-cancel").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}

#[test]
fn footer_adjustment_menu_takes_keyboard_focus_without_nudging_the_layer() {
    let original = Editor::with_test_document();
    let document = original.session().document.clone();
    let (mut cx, view) = quickgui::Application::new()
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(key_bindings())
        .into_test_context(
            quickgui::WindowOptions::new("Footer adjustment menu").size(1500., 900.),
            original,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "layer-adjustment-menu").unwrap();
    assert_eq!(
        cx.focused(window).unwrap(),
        Some(quickgui::ElementId::from("application-menu-items"))
    );
    cx.simulate_keystrokes(window, "home down enter").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document.layers.len(), 2);
        assert_eq!(e.session().document.layers[0], document.layers[0]);
        assert_eq!(
            e.adjustment_edit.as_ref().unwrap().settings.kind,
            Kind::Levels
        );
    })
    .unwrap();
    cx.simulate_keystrokes(window, "escape").unwrap();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        document
    );
}
use quickgui::{Application, Keystroke, WindowOptions};

#[test]
fn pointer_opened_menus_wait_for_hover_while_keyboard_opening_selects_first() {
    let (mut cx, view) = Application::new()
        .bind_keys(quickgui::menubar_key_bindings())
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(key_bindings())
        .into_test_context(
            WindowOptions::new("Menu highlighting").size(1280., 900.),
            Editor::with_test_document(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, Menubar::new("application-menu").item_id(0))
        .unwrap();
    assert_eq!(
        cx.read(view, |e| e.menus.popup.active_index()).unwrap(),
        None
    );
    cx.update(view, |e, cx| {
        e.pending = true;
        cx.invalidate();
    })
    .unwrap();
    assert_eq!(
        cx.read(view, |e| e.menus.popup.active_index()).unwrap(),
        None
    );
    cx.update(view, |e, cx| {
        e.pending = false;
        cx.invalidate();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "down").unwrap();
    assert_eq!(
        cx.read(view, |e| e.menus.popup.active_index()).unwrap(),
        Some(0)
    );
    cx.simulate_keystrokes(window, "escape alt-f").unwrap();
    assert_eq!(
        cx.read(view, |e| e.menus.popup.active_index()).unwrap(),
        Some(0)
    );
}

#[test]
fn open_menus_refresh_labels_availability_and_submenus_without_losing_enabled_highlight() {
    let (mut cx, view) = Application::new()
        .bind_keys(quickgui::menubar_key_bindings())
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(key_bindings())
        .into_test_context(
            WindowOptions::new("Live menus").size(1280., 900.),
            Editor::with_test_document(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, Menubar::new("application-menu").item_id(6))
        .unwrap();
    cx.update(view, |e, cx| {
        let index = e
            .menus
            .popup
            .items()
            .iter()
            .position(|item| item.label().as_ref() == "Hide Layer")
            .unwrap();
        e.menus.popup.highlight(index);
        e.session_mut().document.active_layer_mut().unwrap().visible = false;
        cx.invalidate();
    })
    .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.menus.bar.open_menu(), Some(6));
        assert_eq!(
            e.menus.popup.active_item().unwrap().label().as_ref(),
            "Show Layer"
        );
    })
    .unwrap();
    let show = cx.read(view, |e| e.menus.command_id("Show Layer")).unwrap();
    cx.update(view, |e, cx| {
        e.pending = true;
        cx.invalidate();
    })
    .unwrap();
    assert!(matches!(
        cx.click(window, show),
        Err(quickgui::TestAppError::NotClickable { .. })
    ));
    cx.update(view, |e, cx| {
        e.pending = false;
        cx.invalidate();
    })
    .unwrap();
    cx.click(window, show).unwrap();
    assert!(
        cx.read(view, |e| e
            .session()
            .document
            .active_layer()
            .unwrap()
            .visible)
            .unwrap()
    );
    cx.click(window, Menubar::new("application-menu").item_id(6))
        .unwrap();
    cx.simulate_keystrokes(window, "down").unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::ArrowRight, Modifiers::empty()))
        .unwrap();
    assert!(cx.read(view, |e| e.menus.submenu_anchor.is_some()).unwrap());
    cx.update(view, |e, cx| {
        e.pending = true;
        cx.invalidate();
    })
    .unwrap();
    cx.read(view, |e| {
        assert!(
            e.menus
                .submenu
                .items()
                .iter()
                .all(PopoverMenuItem::is_disabled)
        );
    })
    .unwrap();
    cx.update(view, |e, cx| {
        e.pending = false;
        cx.invalidate();
    })
    .unwrap();
    cx.read(view, |e| {
        assert!(
            e.menus
                .submenu
                .items()
                .iter()
                .all(|item| !item.is_disabled())
        );
    })
    .unwrap();
}
#[test]
fn menu_keyboard_navigation_invokes_commands_and_escape_restores_shortcuts() {
    let (mut cx, view) = Application::new()
        .bind_keys(quickgui::menubar_key_bindings())
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(key_bindings())
        .into_test_context(
            WindowOptions::new("Menus").size(1280., 900.),
            Editor::with_test_document(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        Keystroke::new(Key::Character("l".into()), Modifiers::ALT),
    )
    .unwrap();
    assert_eq!(cx.read(view, |e| e.menus.bar.open_menu()).unwrap(), Some(6));
    assert_eq!(
        cx.focused(window).unwrap(),
        Some(quickgui::ElementId::from("application-menu-items"))
    );
    let new_layer = cx
        .read(view, |e| e.menus.command_id("New Blank Layer"))
        .unwrap();
    cx.click(window, new_layer).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.layers.len())
            .unwrap(),
        2
    );
    cx.simulate_keystroke(
        window,
        Keystroke::new(Key::Function(10), Modifiers::empty()),
    )
    .unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::ArrowRight, Modifiers::empty()))
        .unwrap();
    assert_eq!(cx.read(view, |e| e.menus.bar.open_menu()).unwrap(), Some(1));
    cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
        .unwrap();
    assert!(
        cx.read(view, |e| e.menus.bar.open_menu().is_none())
            .unwrap()
    );
    cx.simulate_keystroke(
        window,
        Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.layers.len())
            .unwrap(),
        1
    );
}

#[test]
fn adjustment_submenu_keyboard_returns_to_parent_and_dispatches() {
    let (mut cx, view) = Application::new()
        .bind_keys(quickgui::menubar_key_bindings())
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(key_bindings())
        .into_test_context(
            WindowOptions::new("Submenus").size(1280., 900.),
            Editor::with_test_document(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        Keystroke::new(Key::Character("l".into()), Modifiers::ALT),
    )
    .unwrap();
    for key in [
        Key::ArrowRight,
        Key::ArrowLeft,
        Key::Enter,
        Key::Escape,
        Key::ArrowRight,
    ] {
        let opens = matches!(key, Key::ArrowRight | Key::Enter);
        cx.simulate_keystroke(window, Keystroke::new(key.clone(), Modifiers::empty()))
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e.menus.submenu_anchor.is_some()).unwrap(),
            opens
        );
        assert_eq!(cx.read(view, |e| e.menus.bar.open_menu()).unwrap(), Some(6));
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(quickgui::ElementId::from(if opens {
                "application-submenu-items"
            } else {
                "application-menu-items"
            })),
            "after {key:?}"
        );
    }
    cx.simulate_keystroke(window, Keystroke::new(Key::ArrowDown, Modifiers::empty()))
        .unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
        .unwrap();
    assert!(
        cx.read(view, |e| e.menus.bar.open_menu().is_none()
            && e.menus.submenu_anchor.is_none())
            .unwrap()
    );
    assert_eq!(
        cx.read(view, |e| e
            .adjustment_edit
            .as_ref()
            .map(|a| a.settings.kind))
            .unwrap(),
        Some(Kind::Levels)
    );
}

#[test]
fn contextual_commands_and_view_checkmarks_follow_document_state() {
    let mut editor = Editor::with_test_document();
    let item = |menu: &PopoverMenu, label: &str| {
        menu.items()
            .iter()
            .find(|i| i.label().as_ref() == label)
            .unwrap()
            .clone()
    };
    let layer_menu = editor.build_menu(6);
    assert!(item(&layer_menu, "Edit Adjustment…").is_disabled());
    assert!(item(&layer_menu, "Move Out of Folder").is_disabled());
    assert!(item(&layer_menu, "Merge Down").is_disabled());
    assert!(!item(&layer_menu, "Hide Layer").is_disabled());
    let view_menu = editor.build_menu(2);
    assert_eq!(
        item(&view_menu, "Pixel Grid (800% and above)").checked(),
        Some(editor.tools.pixel_grid)
    );
    assert_eq!(
        item(&view_menu, "Show Transform Controls").checked(),
        Some(editor.tools.show_transform_controls)
    );
    editor
        .session_mut()
        .document
        .active_layer_mut()
        .unwrap()
        .visible = false;
    assert!(!item(&editor.build_menu(6), "Show Layer").is_disabled());
    editor.session_mut().document.selection = Some(compositor::selection::Selection::rectangle(
        2,
        2,
        [0., 0.],
        [2., 2.],
        false,
    ));
    let layer_menu = editor.build_menu(6);
    assert!(item(&layer_menu, "Layer via Copy").is_disabled());
    assert!(item(&layer_menu, "Transform Layer").is_disabled());
    let mut doc = Document::new(2, 2).unwrap();
    compositor::edits::fill(&mut doc, [80, 140, 210, 255], false, false).unwrap();
    doc.selection = Some(compositor::selection::Selection::rectangle(
        2,
        2,
        [0., 0.],
        [2., 2.],
        false,
    ));
    let active = doc.active.unwrap();
    editor.tabs = vec![Session::new(doc, None).into()];
    assert!(!item(&editor.build_menu(6), "Layer via Copy").is_disabled());
    assert!(!item(&editor.build_menu(6), "Transform Selection").is_disabled());
    let doc = &mut editor.session_mut().document;
    doc.add(compositor::document::Layer::blank("Second", 2, 2))
        .unwrap();
    doc.select(active, true);
    assert!(!item(&editor.build_menu(6), "Transform Layer").is_disabled());
    let doc = &mut editor.session_mut().document;
    doc.select(active, false);
    doc.selection = Some(compositor::selection::Selection::rectangle(
        2,
        2,
        [0., 0.],
        [0., 0.],
        false,
    ));
    assert!(!item(&editor.build_menu(6), "Transform Layer").is_disabled());
}

#[test]
fn selection_menu_reuses_saved_amount_and_visibility_is_undoable() {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(30, 30).unwrap(), None).into()];
    editor.session_mut().document.selection = Some(compositor::selection::Selection::rectangle(
        30,
        30,
        [10., 10.],
        [20., 20.],
        false,
    ));
    editor.tools.selection_expand_amount = 3;
    let expected = editor
        .session()
        .document
        .selection
        .as_ref()
        .unwrap()
        .resized(3, 30, 30)
        .unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Context menus").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, Menubar::new("application-menu").item_id(3))
        .unwrap();
    let expand = cx
        .read(view, |e| e.menus.command_id("Expand by 3 px"))
        .unwrap();
    cx.click(window, expand).unwrap();
    cx.read(view, |e| {
        assert!(e.modal.is_none());
        assert_eq!(e.session().document.selection.as_ref(), Some(&expected));
        assert_eq!(e.session().undo_label(), Some("Expand Selection"));
    })
    .unwrap();
    cx.click(window, Menubar::new("application-menu").item_id(6))
        .unwrap();
    let hide = cx.read(view, |e| e.menus.command_id("Hide Layer")).unwrap();
    cx.click(window, hide).unwrap();
    assert!(
        !cx.read(view, |e| e
            .session()
            .document
            .active_layer()
            .unwrap()
            .visible)
            .unwrap()
    );
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert!(
        cx.read(view, |e| e
            .session()
            .document
            .active_layer()
            .unwrap()
            .visible)
            .unwrap()
    );
}

#[test]
fn native_menu_order_and_mnemonics_match_the_source_commands() {
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .bind_keys(quickgui::menubar_key_bindings())
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(key_bindings())
        .into_test_context(
            WindowOptions::new("Source menu order").size(1280., 900.),
            Editor::with_test_document(),
        )
        .unwrap();
    let window = view.window_handle();
    let bar = Menubar::new("application-menu");
    let mut previous_right = 0.;
    for (index, title, mnemonic, command) in [
        (0, "File", "alt-f", "New Canvas…"),
        (1, "Edit", "alt-e", "Undo"),
        (2, "View", "alt-v", "Fit Canvas"),
        (3, "Select", "alt-s", "All"),
        (4, "Image", "alt-i", "Curves…"),
        (5, "Filter", "alt-t", "Gaussian Blur…"),
        (6, "Layer", "alt-l", "New Adjustment Layer"),
        (7, "Help", "alt-h", "Keyboard Shortcuts…"),
    ] {
        assert_eq!(TITLES[index], title);
        let bounds = cx.element_bounds(window, bar.item_id(index)).unwrap();
        assert!(bounds.x >= previous_right);
        previous_right = bounds.x + bounds.width;
        cx.click(window, bar.item_id(index)).unwrap();
        cx.read(view, |e| {
            assert_eq!(e.menus.popup.items()[0].label().as_ref(), command);
        })
        .unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        cx.simulate_keystrokes(window, mnemonic).unwrap();
        cx.read(view, |e| {
            assert_eq!(e.menus.bar.open_menu(), Some(index));
            assert_eq!(e.menus.popup.items()[0].label().as_ref(), command);
        })
        .unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
    }
    cx.simulate_keystrokes(window, "f10 right right").unwrap();
    assert_eq!(cx.read(view, |e| e.menus.bar.open_menu()).unwrap(), Some(2));
    cx.simulate_keystrokes(window, "right").unwrap();
    assert_eq!(
        cx.read(view, |e| e.menus.popup.items()[0].label().to_string())
            .unwrap(),
        "All"
    );
    cx.simulate_keystrokes(window, "left").unwrap();
    assert_eq!(
        cx.read(view, |e| e.menus.popup.items()[0].label().to_string())
            .unwrap(),
        "Fit Canvas"
    );
}
