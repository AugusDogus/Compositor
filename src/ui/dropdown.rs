//! Fixed-choice menus anchored inside the editor window on both X11 and Wayland.
//! QuickGUI's native Select windows are ordinary toplevels on Wayland and cannot
//! be positioned by the client. Reuse its selection, menu navigation and popover parts.
use super::*;
use quickgui::{
    ElementId, PickerError, PickerItem, Popover, PopoverKind, PopoverMenu, PopoverMenuItem,
    SelectOptionState, SelectPopoverLayout, SelectState, StateAccessor,
};

pub(super) struct Dropdown<T> {
    selection: SelectState<T>,
    open: Option<PopoverMenu>,
    layout: SelectPopoverLayout,
}

impl<T> Dropdown<T> {
    pub(super) fn new(
        items: impl IntoIterator<Item = PickerItem<T>>,
    ) -> std::result::Result<Self, PickerError> {
        Ok(Self {
            selection: SelectState::new(items)?,
            open: None,
            layout: SelectPopoverLayout::default(),
        })
    }
    pub(super) fn with_layout(mut self, layout: SelectPopoverLayout) -> Self {
        self.layout = layout;
        self
    }
    pub(super) fn items(&self) -> &[PickerItem<T>] {
        self.selection.items()
    }
    pub(super) fn selected_value(&self) -> Option<&T> {
        self.selection.selected_value()
    }
    pub(super) fn value_text(&self) -> Option<Arc<str>> {
        self.selection.value_text()
    }
    pub(super) fn select_id(&mut self, id: impl Into<ElementId>) -> bool {
        self.selection.select_id(id)
    }
    pub(super) fn clear_selection(&mut self) -> bool {
        self.selection.clear_selection()
    }
    pub(super) fn is_open(&self) -> bool {
        self.open.is_some()
    }
    pub(super) fn close(&mut self, cx: &mut EventContext) {
        if self.open.take().is_some() {
            cx.invalidate();
        }
    }
    pub(super) fn dismiss(&mut self) {
        self.open = None;
    }
    fn show(&mut self, id: ElementId, cx: &mut EventContext) {
        if self.open.is_some() {
            self.close(cx);
            return;
        }
        let mut menu = PopoverMenu::new(self.items().iter().enumerate().map(|(index, item)| {
            PopoverMenuItem::action(format!("choice-{index}"), item.label().clone(), ())
                .disabled(item.is_disabled())
        }))
        .expect("Dropdown options have unique IDs");
        if let Some(index) = self.selection.selected_source_index() {
            menu.highlight(index);
        }
        if menu.active_index().is_none() {
            menu.select_first();
        }
        self.open = Some(menu);
        cx.focus(quickgui::FocusHandle::new(SelectState::<T>::surface_id(id)));
        cx.invalidate();
    }
}

impl<T: Clone + 'static> Dropdown<T> {
    fn choose(&mut self, index: usize, cx: &mut EventContext) -> Option<T> {
        self.open.as_ref()?;
        let value = self
            .items()
            .get(index)
            .filter(|item| !item.is_disabled())?
            .value()
            .clone();
        self.selection.select_source(index);
        self.close(cx);
        Some(value)
    }

    pub(super) fn element<Change>(
        &self,
        cx: &mut ViewContext<'_, Editor>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: fn(&mut Editor) -> &mut Self,
        trigger: Element,
        change: Change,
    ) -> Element
    where
        Change: Fn(&mut Editor, T, &mut EventContext) + Clone + 'static,
    {
        self.element_with(cx, id, label, StateAccessor::from(access), trigger, change)
    }

    pub(super) fn element_with<Change>(
        &self,
        cx: &mut ViewContext<'_, Editor>,
        id: impl Into<ElementId>,
        label: impl Into<Arc<str>>,
        access: StateAccessor<Editor, Self>,
        trigger: Element,
        change: Change,
    ) -> Element
    where
        Change: Fn(&mut Editor, T, &mut EventContext) + Clone + 'static,
    {
        let id = id.into();
        let label = label.into();
        let click_access = access.clone();
        let key_access = access.clone();
        let trigger = self
            .selection
            .trigger_with(id, label.clone(), trigger)
            .accessibility_expanded(self.is_open())
            .on_click(cx.listener(id, move |this, cx| click_access.get(this).show(id, cx)))
            .on_key_down(cx.key_down_listener(id, move |this, event, cx| {
                if matches!(
                    event.key,
                    Key::ArrowUp | Key::ArrowDown | Key::Enter | Key::Space
                ) {
                    key_access.get(this).show(id, cx);
                    cx.prevent_default();
                    cx.stop_propagation();
                }
            }));
        let Some(menu) = &self.open else {
            return trigger;
        };
        let surface = SelectState::<T>::surface_id(id);
        let mut content = super::menus::style::surface()
            // A sheet already occupies the overlay plane. Raise its nested menu
            // above later sheet fields for both painting and pointer dispatch.
            .z_index(1)
            .p(5.)
            .gap(1.)
            .w(self.layout.width)
            .max_h((cx.size().height - 32.).max(self.layout.row_height))
            .overflow_y_scroll()
            .flex_col();
        for (index, item) in self.items().iter().enumerate() {
            let state = SelectOptionState {
                source_index: index,
                active: menu.active_index() == Some(index),
                selected: self.selection.is_source_selected(index),
                disabled: item.is_disabled(),
            };
            let row_id = self
                .selection
                .option_id_for_source(id, index)
                .expect("Declared dropdown option exists");
            let choose_access = access.clone();
            let choose_change = change.clone();
            let hover_access = access.clone();
            content = content.child(
                SelectState::<T>::item_with(
                    row_id,
                    item.label().clone(),
                    state,
                    super::menus::style::choice(
                        item.label().clone(),
                        quickgui::PopoverMenuItemState {
                            highlighted: state.active,
                            disabled: state.disabled,
                            checked: Some(state.selected),
                            has_submenu: false,
                        },
                    )
                    .h(self.layout.row_height),
                )
                .on_click(cx.listener(row_id, move |this, cx| {
                    if let Some(value) = choose_access.get(this).choose(index, cx) {
                        cx.focus(quickgui::FocusHandle::new(id));
                        choose_change(this, value, cx);
                    }
                    cx.stop_propagation();
                }))
                .on_hover(cx.hover_listener(row_id, move |this, hovered, cx| {
                    if *hovered
                        && let Some(menu) = &mut hover_access.get(this).open
                        && menu.highlight(index)
                    {
                        cx.invalidate();
                    }
                })),
            );
        }
        let dismiss_access = access.clone();
        let key_access = access;
        let mut popup = Popover::new(id, surface, true)
            .kind(PopoverKind::Menu)
            .placement(self.layout.placement)
            .side_offset(self.layout.anchor_gap)
            .initial_focus(surface)
            .surface_with(content)
            .on_dismiss(cx.dismiss_listener(surface, move |this, cx| {
                dismiss_access.get(this).close(cx);
                cx.focus(quickgui::FocusHandle::new(id));
            }))
            .on_key_down(cx.key_down_listener(surface, move |this, event, cx| {
                let state = key_access.get(this);
                let Some(menu) = &mut state.open else { return };
                match event.key {
                    Key::Enter | Key::Space => {
                        if let Some(index) = menu.active_index()
                            && let Some(value) = state.choose(index, cx)
                        {
                            cx.focus(quickgui::FocusHandle::new(id));
                            change(this, value, cx);
                        }
                    }
                    Key::Escape | Key::Tab => {
                        state.close(cx);
                        cx.focus(quickgui::FocusHandle::new(id));
                    }
                    Key::ArrowUp => {
                        menu.select_previous();
                    }
                    Key::ArrowDown => {
                        menu.select_next();
                    }
                    Key::Home | Key::PageUp => {
                        menu.select_first();
                    }
                    Key::End | Key::PageDown => {
                        menu.select_last();
                    }
                    _ if !event
                        .modifiers
                        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) =>
                    {
                        if let Key::Character(text) = event.key_char.as_ref().unwrap_or(&event.key)
                        {
                            menu.typeahead(text, std::time::Instant::now());
                        }
                    }
                    _ => {}
                }
                cx.invalidate();
                cx.prevent_default();
                cx.stop_propagation();
            }));
        if let Some(active) = menu.active_index()
            && let Some(row) = self.selection.option_id_for_source(id, active)
        {
            popup = popup.accessibility_active_descendant(row);
        }
        trigger.child(SelectState::<T>::popup_with(
            id,
            label,
            self.items().len(),
            false,
            popup,
        ))
    }
}
