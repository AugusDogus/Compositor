use std::{sync::Arc, time::Duration};

use crate::{AccessibilityRole, Element, ElementId, MAX_VALIDATION_MESSAGE_BYTES};

/// Longest revalidation debounce one field may declare.
pub const MAX_FIELD_VALIDATION_DEBOUNCE: Duration = Duration::from_secs(10);

const FIELD_ROOT_ID_TAG: u64 = 0x6669_656c_645f_726f;
const FIELD_LABEL_ID_TAG: u64 = 0x6669_656c_645f_6c61;
const FIELD_DESCRIPTION_ID_TAG: u64 = 0x6669_656c_645f_6465;
const FIELD_ERROR_ID_TAG: u64 = 0x6669_656c_645f_6572;
const FIELD_ITEM_ID_TAG: u64 = 0x6669_656c_645f_6974;
const FIELD_VALIDITY_ID_TAG: u64 = 0x6669_656c_645f_7661;
const FIELDSET_LEGEND_ID_TAG: u64 = 0x6669_656c_6473_6c67;
const FIELDSET_DESCRIPTION_ID_TAG: u64 = 0x6669_656c_6473_6465;

/// When a field's controlled validity is expected to be recomputed.
///
/// This is Base UI's `validationMode`. QuickGUI never runs the application's validation rule
/// itself — the rule is application logic and may reach a database — so the mode is a contract the
/// field publishes and [`Field::should_validate`] answers against the event that just happened.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FieldValidationMode {
    /// Validate only when the enclosing form is submitted.
    #[default]
    OnSubmit,
    /// Validate when the control loses focus, and on every submit.
    OnBlur,
    /// Validate on every change, and on blur and submit.
    OnChange,
}

/// What just happened to a field's value or focus.
///
/// Pass one to [`Field::should_validate`] from the listener that already handles it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldValidationTrigger {
    /// The controlled value changed.
    Change,
    /// The control lost focus.
    Blur,
    /// The enclosing form was submitted.
    Submit,
}

/// Caller-owned state projected consistently across every unstyled field part.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FieldState {
    pub disabled: bool,
    pub invalid: bool,
    pub required: bool,
    pub touched: bool,
    pub dirty: bool,
    pub filled: bool,
}

impl FieldState {
    pub const fn is_valid(self) -> bool {
        !self.invalid
    }
}

/// Controlled, unstyled labeling and validation composition for one form control.
///
/// The application owns the value, validation rule, every rendered part, layout, typography,
/// colors, borders, focus treatment, and motion. QuickGUI supplies stable part identities,
/// click-to-activate label behavior, exact accessibility relationships, required/invalid/disabled
/// control state, and one bounded native validation message.
///
/// This descriptor retains no task, timer, observer, item registry, or idle scheduler source.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "a Field descriptor has no effect until its parts are mounted"]
pub struct Field {
    control_id: ElementId,
    state: FieldState,
    validation_message: Option<Arc<str>>,
    validation_message_truncated: bool,
    validation_mode: FieldValidationMode,
    validation_debounce: Duration,
}

impl Field {
    pub fn new(control_id: impl Into<ElementId>) -> Self {
        Self {
            control_id: control_id.into(),
            state: FieldState::default(),
            validation_message: None,
            validation_message_truncated: false,
            validation_mode: FieldValidationMode::OnSubmit,
            validation_debounce: Duration::ZERO,
        }
    }

    pub const fn control_id(&self) -> ElementId {
        self.control_id
    }

    pub fn root_id(&self) -> ElementId {
        derived_field_id(self.control_id, FIELD_ROOT_ID_TAG)
    }

    pub fn label_id(&self) -> ElementId {
        derived_field_id(self.control_id, FIELD_LABEL_ID_TAG)
    }

    pub fn description_id(&self) -> ElementId {
        derived_field_id(self.control_id, FIELD_DESCRIPTION_ID_TAG)
    }

    pub fn error_id(&self) -> ElementId {
        derived_field_id(self.control_id, FIELD_ERROR_ID_TAG)
    }

    /// Stable identity of the item wrapper around one label/control/description row.
    pub fn item_id(&self) -> ElementId {
        derived_field_id(self.control_id, FIELD_ITEM_ID_TAG)
    }

    /// Stable identity of the validity part.
    pub fn validity_id(&self) -> ElementId {
        derived_field_id(self.control_id, FIELD_VALIDITY_ID_TAG)
    }

    pub const fn state(&self) -> FieldState {
        self.state
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.state.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.state.invalid = invalid;
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.state.required = required;
        self
    }

    pub fn touched(mut self, touched: bool) -> Self {
        self.state.touched = touched;
        self
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.state.dirty = dirty;
        self
    }

    pub fn filled(mut self, filled: bool) -> Self {
        self.state.filled = filled;
        self
    }

    /// Declare when the controlled validity is expected to be recomputed, Base UI's
    /// `validationMode`.
    pub const fn validation_mode(mut self, mode: FieldValidationMode) -> Self {
        self.validation_mode = mode;
        self
    }

    /// Wait this long after a change before revalidating, Base UI's `validationDebounceTime`.
    ///
    /// The interval is an exact one-shot deadline the application schedules with
    /// [`crate::AsyncViewContext::sleep`]; QuickGUI never polls, and a field that is not being
    /// typed into owns nothing. It applies to [`FieldValidationTrigger::Change`] only — a blur or
    /// a submit is a deliberate boundary and validates immediately. Clamped to
    /// [`MAX_FIELD_VALIDATION_DEBOUNCE`].
    pub fn validation_debounce(mut self, debounce: Duration) -> Self {
        self.validation_debounce = debounce.min(MAX_FIELD_VALIDATION_DEBOUNCE);
        self
    }

    pub const fn validation_mode_value(&self) -> FieldValidationMode {
        self.validation_mode
    }

    pub const fn validation_debounce_value(&self) -> Duration {
        self.validation_debounce
    }

    /// Whether the declared mode expects this event to recompute validity.
    ///
    /// A submit always validates; a blur validates on `OnBlur` and `OnChange`; a change validates
    /// only on `OnChange`.
    pub const fn should_validate(&self, trigger: FieldValidationTrigger) -> bool {
        match (self.validation_mode, trigger) {
            (_, FieldValidationTrigger::Submit) => true,
            (FieldValidationMode::OnSubmit, _) => false,
            (FieldValidationMode::OnBlur, FieldValidationTrigger::Blur) => true,
            (FieldValidationMode::OnBlur, FieldValidationTrigger::Change) => false,
            (FieldValidationMode::OnChange, _) => true,
        }
    }

    /// How long to wait before recomputing validity for this event, when it validates at all.
    ///
    /// Returns `None` when the declared mode ignores the event, and `Some(Duration::ZERO)` when it
    /// validates immediately, so a listener can branch once instead of re-deriving the policy.
    pub const fn validation_delay(&self, trigger: FieldValidationTrigger) -> Option<Duration> {
        if !self.should_validate(trigger) {
            return None;
        }
        match trigger {
            FieldValidationTrigger::Change => Some(self.validation_debounce),
            FieldValidationTrigger::Blur | FieldValidationTrigger::Submit => Some(Duration::ZERO),
        }
    }

    /// Retain the message used by form reports and native accessibility.
    ///
    /// The visible error part remains application-owned and can use different presentation copy.
    pub fn validation_message(mut self, message: impl Into<Arc<str>>) -> Self {
        let (message, truncated) = bounded_validation_message(message.into());
        self.validation_message = message;
        self.validation_message_truncated = truncated;
        self
    }

    pub fn validation_message_text(&self) -> Option<&str> {
        self.validation_message.as_deref()
    }

    pub const fn validation_message_is_truncated(&self) -> bool {
        self.validation_message_truncated
    }

    /// Decorate an application-owned structural root without adding role or appearance.
    pub fn root_with(&self, root: Element) -> Element {
        root.id(self.root_id())
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(&self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate a visible label and give it native label-to-control activation.
    ///
    /// Use [`Self::passive_label_with`] for button-like controls whose label should name but not
    /// activate them.
    pub fn label_with(&self, label: Element) -> Element {
        let label = self.passive_label_with(label);
        if self.state.disabled {
            label.disabled(true)
        } else {
            label.activate_target_on_click(self.control_id)
        }
    }
    /// Create the unstyled label part. Use [`Self::label_with`] to supply an existing element.
    pub fn label(&self) -> Element {
        self.label_with(crate::div())
    }

    /// Decorate a label relationship without forwarding pointer activation to the control.
    pub fn passive_label_with(&self, label: Element) -> Element {
        label
            .id(self.label_id())
            .accessibility_role(AccessibilityRole::Label)
            .app_region_no_drag()
    }
    /// Create the unstyled passive label part. Use [`Self::passive_label_with`] to supply an existing element.
    pub fn passive_label(&self) -> Element {
        self.passive_label_with(crate::div())
    }

    /// Decorate the application-owned control with field state and mounted relationships.
    pub fn control_with(&self, control: Element) -> Element {
        let disabled = control.accessibility.disabled || self.state.disabled;
        let required = control.accessibility.required || self.state.required;
        let mut control = control
            .id(self.control_id)
            .disabled(disabled)
            .required(required)
            .invalid(self.state.invalid)
            .accessibility_labelled_by(self.label_id())
            .app_region_no_drag();
        control = if self.state.invalid {
            control.accessibility_described_by_pair(self.description_id(), self.error_id())
        } else {
            control.accessibility_described_by(self.description_id())
        };
        if let Some(message) = self.validation_message.clone() {
            control =
                control.validation_message_retained(message, self.validation_message_truncated);
        }
        control
    }
    /// Create the unstyled control part. Use [`Self::control_with`] to supply an existing element.
    pub fn control(&self) -> Element {
        self.control_with(crate::div())
    }

    /// Decorate the caller-owned wrapper around one label/control/description row.
    ///
    /// Base UI's Field.Item groups the parts of a single field inside a larger fieldset so the row
    /// can be styled and laid out as one unit. QuickGUI supplies the stable identity and propagates
    /// the field's disabled state; layout and appearance stay application-owned.
    pub fn item_with(&self, item: Element) -> Element {
        let item = item.id(self.item_id()).app_region_no_drag();
        if self.state.disabled {
            item.disabled(true)
        } else {
            item
        }
    }
    /// Create the unstyled item part. Use [`Self::item_with`] to supply an existing element.
    pub fn item(&self) -> Element {
        self.item_with(crate::div())
    }

    /// Decorate a caller-owned part that is shown only for a chosen validity, Base UI's
    /// Field.Validity.
    ///
    /// `visible` is the application's own predicate over [`Self::state`] — "invalid and touched",
    /// "valid and dirty", whatever the product means. QuickGUI removes the part from layout, paint,
    /// input, and the accessibility tree when the predicate is false, exactly as
    /// [`Self::error_with`] does, so an unmatched validity costs nothing.
    pub fn validity_with(&self, visible: bool, validity: Element) -> Element {
        validity
            .id(self.validity_id())
            .accessibility_role(AccessibilityRole::Label)
            .when(!visible, Element::hidden)
    }
    /// Create the unstyled validity part. Use [`Self::validity_with`] to supply an existing element.
    pub fn validity(&self, visible: bool) -> Element {
        self.validity_with(visible, crate::div())
    }

    /// Decorate visible supplementary help for the control.
    pub fn description_with(&self, description: Element) -> Element {
        description
            .id(self.description_id())
            .accessibility_role(AccessibilityRole::Label)
    }
    /// Create the unstyled description part. Use [`Self::description_with`] to supply an existing element.
    pub fn description(&self) -> Element {
        self.description_with(crate::div())
    }

    /// Decorate a visible error and remove it from layout while the controlled field is valid.
    pub fn error_with(&self, error: Element) -> Element {
        error
            .id(self.error_id())
            .accessibility_role(AccessibilityRole::Label)
            .when(!self.state.invalid, Element::hidden)
    }
    /// Create the unstyled error part. Use [`Self::error_with`] to supply an existing element.
    pub fn error(&self) -> Element {
        self.error_with(crate::div())
    }
}

/// Controlled, unstyled group semantics for related fields.
///
/// Use [`Self::field`] to propagate disabled state into each nested field without a registry or
/// inherited runtime context. Direct custom controls can use [`Self::control_with`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Fieldset descriptor has no effect until its parts are mounted"]
pub struct Fieldset {
    id: ElementId,
    disabled: bool,
}

impl Fieldset {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            disabled: false,
        }
    }

    pub const fn id(self) -> ElementId {
        self.id
    }

    pub fn legend_id(self) -> ElementId {
        derived_field_id(self.id, FIELDSET_LEGEND_ID_TAG)
    }

    pub fn description_id(self) -> ElementId {
        derived_field_id(self.id, FIELDSET_DESCRIPTION_ID_TAG)
    }

    pub const fn is_disabled(self) -> bool {
        self.disabled
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Create one nested field with group disabled state already applied.
    pub fn field(self, control_id: impl Into<ElementId>) -> Field {
        Field::new(control_id).disabled(self.disabled)
    }

    /// Decorate an application-owned group root with legend and description relationships.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.id)
            .accessibility_role(AccessibilityRole::Group)
            .accessibility_labelled_by(self.legend_id())
            .accessibility_described_by(self.description_id())
            .disabled(self.disabled)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    pub fn legend_with(self, legend: Element) -> Element {
        legend
            .id(self.legend_id())
            .accessibility_role(AccessibilityRole::Label)
    }
    /// Create the unstyled legend part. Use [`Self::legend_with`] to supply an existing element.
    pub fn legend(self) -> Element {
        self.legend_with(crate::div())
    }

    pub fn description_with(self, description: Element) -> Element {
        description
            .id(self.description_id())
            .accessibility_role(AccessibilityRole::Label)
    }
    /// Create the unstyled description part. Use [`Self::description_with`] to supply an existing element.
    pub fn description(self) -> Element {
        self.description_with(crate::div())
    }

    /// Apply fieldset disabled state to an application-owned direct control.
    pub fn control_with(self, control: Element) -> Element {
        if self.disabled {
            control.disabled(true)
        } else {
            control
        }
    }
    /// Create the unstyled control part. Use [`Self::control_with`] to supply an existing element.
    pub fn control(self) -> Element {
        self.control_with(crate::div())
    }
}

fn derived_field_id(parent: ElementId, tag: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(17);
    }
    ElementId::new(hash)
}

fn bounded_validation_message(message: Arc<str>) -> (Option<Arc<str>>, bool) {
    if message.is_empty() {
        return (None, false);
    }
    if message.len() <= MAX_VALIDATION_MESSAGE_BYTES {
        return (Some(message), false);
    }
    let mut end = MAX_VALIDATION_MESSAGE_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    (Some(Arc::from(&message[..end])), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Color, Insets, TestAppContext, View, ViewContext, checkbox, div, text, text_input,
    };

    #[test]
    fn parts_preserve_application_appearance_and_wire_exact_state() {
        let field = Field::new("name")
            .required(true)
            .invalid(true)
            .touched(true)
            .dirty(true)
            .filled(false)
            .validation_message("Name is required");
        assert!(!field.state().is_valid());
        let root = field.root_with(div().w(321.0).bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some(field.root_id()));
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let label = field.label_with(text("Name").text_lg());
        assert_eq!(label.explicit_id, Some(field.label_id()));
        assert_eq!(label.accessibility.role, AccessibilityRole::Label);
        assert_eq!(label.activation_target, Some(field.control_id()));
        assert!(label.clickable);
        assert!(!label.focusable);

        let control = field.control_with(text_input("").border(3.0, Color::rgb8(4, 5, 6)));
        assert_eq!(control.explicit_id, Some(field.control_id()));
        assert!(control.accessibility.required);
        assert!(control.accessibility.invalid);
        assert_eq!(
            control.accessibility.validation_message.as_deref(),
            Some("Name is required")
        );
        assert_eq!(
            control.accessibility.relations.labelled_by(),
            Some(field.label_id())
        );
        assert_eq!(
            control.accessibility.relations.described_by(),
            Some(field.description_id())
        );
        assert_eq!(
            control.accessibility.relations.described_by_secondary(),
            Some(field.error_id())
        );
        assert_eq!(control.visual.border_widths, Insets::all(3.0));

        assert!(
            !field
                .description_with(text("Public profile"))
                .is_display_none()
        );
        assert!(!field.error_with(text("Required")).is_display_none());
        assert!(
            Field::new("valid")
                .error_with(text("Not mounted"))
                .is_display_none()
        );
    }

    #[test]
    fn messages_are_utf8_bounded_and_part_ids_are_stable_and_distinct() {
        let message = "é".repeat(MAX_VALIDATION_MESSAGE_BYTES);
        let field = Field::new(0_u64).validation_message(message);
        assert!(field.validation_message_is_truncated());
        let retained = field.validation_message_text().unwrap();
        assert!(retained.len() <= MAX_VALIDATION_MESSAGE_BYTES);
        assert!(std::str::from_utf8(retained.as_bytes()).is_ok());

        let ids = [
            field.control_id(),
            field.root_id(),
            field.label_id(),
            field.description_id(),
            field.error_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(id.as_u64(), u64::MAX);
            assert!(!ids[..index].contains(id));
        }
    }

    #[derive(Default)]
    struct FieldView {
        value: Arc<str>,
        checked: bool,
    }

    impl View for FieldView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl crate::IntoElement {
            let edit = cx.input_listener("name", |this, value, cx| {
                this.value = Arc::from(value);
                cx.invalidate();
            });
            let toggle = cx.listener("agree", |this, cx| {
                this.checked = !this.checked;
                cx.invalidate();
            });
            let name = Field::new("name")
                .required(true)
                .invalid(self.value.is_empty())
                .validation_message("Name is required");
            let agree = Field::new("agree");
            let disabled = Fieldset::new("disabled-group").disabled(true);
            let blocked = disabled.field("blocked");
            div()
                .child(name.label_with(text("Name")))
                .child(name.control_with(text_input(self.value.clone()).on_input(edit)))
                .child(name.description_with(text("Visible publicly")))
                .child(name.error_with(text("Name is required")))
                .child(agree.label_with(text("Agree")))
                .child(agree.control_with(checkbox(self.checked).on_click(toggle)))
                .child(disabled.root_with(div()).children([
                    disabled.legend_with(text("Disabled group")),
                    blocked.label_with(text("Blocked")),
                    blocked.control_with(text_input("")),
                ]))
        }
    }

    #[test]
    fn labels_focus_or_activate_controls_and_disabled_groups_do_not() {
        let (mut cx, view) = TestAppContext::new(FieldView::default()).unwrap();
        let window = view.window_handle();
        let name = Field::new("name")
            .required(true)
            .invalid(true)
            .validation_message("Name is required");
        let agree = Field::new("agree");

        cx.click(window, name.label_id()).unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(name.control_id()));
        cx.click(window, agree.label_id()).unwrap();
        assert!(cx.read(view, |view| view.checked).unwrap());

        let blocked = Fieldset::new("disabled-group")
            .disabled(true)
            .field("blocked");
        assert!(cx.click(window, blocked.label_id()).is_err());

        let update = cx.accessibility_update(window).unwrap();
        let control = update
            .nodes
            .iter()
            .find_map(|(id, node)| (id.0 == name.control_id().as_u64()).then_some(node))
            .expect("field control accessibility node");
        assert!(control.is_required());
        assert_eq!(control.invalid(), Some(accesskit::Invalid::True));
        assert_eq!(
            control.labelled_by(),
            &[accesskit::NodeId(name.label_id().as_u64())]
        );
        assert_eq!(
            control.described_by(),
            &[
                accesskit::NodeId(name.description_id().as_u64()),
                accesskit::NodeId(name.error_id().as_u64()),
            ]
        );
        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn fieldset_wires_group_semantics_without_paint() {
        let fieldset = Fieldset::new("billing").disabled(true);
        let root = fieldset.root_with(div().bg(Color::rgb8(9, 8, 7)));
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert!(root.accessibility.disabled);
        assert_eq!(
            root.accessibility.relations.labelled_by(),
            Some(fieldset.legend_id())
        );
        assert_eq!(
            root.accessibility.relations.described_by(),
            Some(fieldset.description_id())
        );
        assert_eq!(root.visual.background, Some(Color::rgb8(9, 8, 7)));
        assert!(fieldset.field("company").state().disabled);
        assert!(fieldset.control_with(text_input("")).accessibility.disabled);
    }

    #[test]
    fn validation_modes_answer_each_trigger_and_debounce_only_changes() {
        let on_submit = Field::new("email");
        assert_eq!(
            on_submit.validation_mode_value(),
            FieldValidationMode::OnSubmit
        );
        assert!(on_submit.should_validate(FieldValidationTrigger::Submit));
        assert!(!on_submit.should_validate(FieldValidationTrigger::Blur));
        assert!(!on_submit.should_validate(FieldValidationTrigger::Change));

        let on_blur = Field::new("email").validation_mode(FieldValidationMode::OnBlur);
        assert!(on_blur.should_validate(FieldValidationTrigger::Submit));
        assert!(on_blur.should_validate(FieldValidationTrigger::Blur));
        assert!(!on_blur.should_validate(FieldValidationTrigger::Change));

        let on_change = Field::new("email")
            .validation_mode(FieldValidationMode::OnChange)
            .validation_debounce(Duration::from_millis(250));
        assert!(on_change.should_validate(FieldValidationTrigger::Change));
        assert_eq!(
            on_change.validation_debounce_value(),
            Duration::from_millis(250)
        );

        // A change waits for the declared debounce; a blur or submit is a deliberate boundary.
        assert_eq!(
            on_change.validation_delay(FieldValidationTrigger::Change),
            Some(Duration::from_millis(250))
        );
        assert_eq!(
            on_change.validation_delay(FieldValidationTrigger::Blur),
            Some(Duration::ZERO)
        );
        assert_eq!(
            on_change.validation_delay(FieldValidationTrigger::Submit),
            Some(Duration::ZERO)
        );
        assert_eq!(
            on_submit.validation_delay(FieldValidationTrigger::Change),
            None
        );
        assert_eq!(
            on_blur.validation_delay(FieldValidationTrigger::Change),
            None
        );

        // The debounce is bounded rather than trusted.
        assert_eq!(
            Field::new("email")
                .validation_debounce(Duration::from_secs(3_600))
                .validation_debounce_value(),
            MAX_FIELD_VALIDATION_DEBOUNCE
        );
        assert_eq!(
            Field::new("email").validation_debounce_value(),
            Duration::ZERO
        );
    }

    #[test]
    fn item_and_validity_parts_carry_identities_and_cost_nothing_when_unmatched() {
        let field = Field::new("email").invalid(true).touched(true);
        let ids = [
            field.root_id(),
            field.label_id(),
            field.description_id(),
            field.error_id(),
            field.item_id(),
            field.validity_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, field.control_id());
            assert!(!ids[..index].contains(id));
        }

        let item = field.item_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(item.explicit_id, Some(field.item_id()));
        assert_eq!(item.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert!(!item.accessibility.disabled);
        // A disabled field propagates into the row it groups.
        assert!(
            Field::new("email")
                .disabled(true)
                .item_with(div())
                .accessibility
                .disabled
        );

        // The application supplies the predicate; QuickGUI only removes the unmatched part.
        let shown = field.validity_with(field.state().invalid && field.state().touched, div());
        assert_eq!(shown.explicit_id, Some(field.validity_id()));
        assert_eq!(shown.accessibility.role, AccessibilityRole::Label);
        assert!(!shown.is_display_none());
        let hidden = field.validity_with(false, div());
        assert!(hidden.is_display_none());
        assert_eq!(hidden.visual.background, None);
    }
}
