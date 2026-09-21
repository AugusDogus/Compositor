use crate::{AccessibilityOrientation, AccessibilityRole, Element, div};

/// Layout axis projected by one [`Separator`].
///
/// The name follows the Base UI and ARIA convention: a *horizontal* separator is a horizontal
/// line that divides vertically stacked content, and a *vertical* separator is a vertical line
/// between horizontally arranged content.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SeparatorOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl SeparatorOrientation {
    const fn accessibility(self) -> AccessibilityOrientation {
        match self {
            Self::Horizontal => AccessibilityOrientation::Horizontal,
            Self::Vertical => AccessibilityOrientation::Vertical,
        }
    }

    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Vertical)
    }
}

/// Copyable declaration for one unstyled semantic separator.
///
/// The application owns the rule's thickness, color, inset, and spacing. QuickGUI supplies only
/// the Separator role, the projected orientation, and the desktop pointer contract, so a divider
/// never joins the Tab sequence and never contributes to a neighbouring accessible name.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
///
/// ```
/// use quickgui::{Color, Separator, SeparatorOrientation, div};
///
/// // The application declares the rule's extent and colour; the descriptor adds only semantics.
/// let rule = Separator::new(SeparatorOrientation::Horizontal)
///     .root_with(div().h(1.0).bg(Color::rgb8(220, 220, 220)));
/// let vertical = Separator::vertical();
/// assert!(vertical.axis().is_vertical());
/// let _ = rule;
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "a Separator descriptor has no effect until its root part is mounted"]
pub struct Separator {
    orientation: SeparatorOrientation,
}

impl Separator {
    /// Declare a separator along `orientation`. Horizontal is the default, matching Base UI.
    pub const fn new(orientation: SeparatorOrientation) -> Self {
        Self { orientation }
    }

    /// Declare a horizontal rule between vertically stacked content.
    pub const fn horizontal() -> Self {
        Self::new(SeparatorOrientation::Horizontal)
    }

    /// Declare a vertical rule between horizontally arranged content.
    pub const fn vertical() -> Self {
        Self::new(SeparatorOrientation::Vertical)
    }

    pub const fn axis(self) -> SeparatorOrientation {
        self.orientation
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.accessibility_role(AccessibilityRole::Separator)
            .accessibility_orientation(self.orientation.accessibility())
            .user_select_none()
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }
}

/// Create an unstyled semantic separator root.
///
/// This shorthand is equivalent to `Separator::new(orientation).root_with(div())`. The caller
/// still declares the rule's extent, because a zero-sized divider is a layout decision rather
/// than a framework default.
pub fn separator(orientation: SeparatorOrientation) -> Element {
    Separator::new(orientation).root_with(div())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Color, CursorStyle, ElementId, IntoElement, TestAppContext, UserSelect, View,
        ViewContext, text,
    };

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let horizontal = Separator::horizontal().root_with(div());
        assert_eq!(horizontal.accessibility.role, AccessibilityRole::Separator);
        assert_eq!(
            horizontal.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );
        assert!(!horizontal.focusable);
        assert_eq!(horizontal.visual.background, None);
        assert_eq!(horizontal.visual.border_color, None);
        assert_eq!(horizontal.cursor_style, Some(CursorStyle::Arrow));
        assert_eq!(horizontal.app_region, Some(AppRegion::NoDrag));
        assert_eq!(horizontal.user_select, UserSelect::None);
        assert!(horizontal.children.is_empty());

        let vertical = Separator::vertical().root_with(div().w(1.0).bg(Color::BLACK));
        assert_eq!(
            vertical.accessibility.orientation,
            Some(AccessibilityOrientation::Vertical)
        );
        assert_eq!(vertical.visual.background, Some(Color::BLACK));
        assert!(Separator::vertical().axis().is_vertical());
        assert!(!Separator::default().axis().is_vertical());
        assert_eq!(Separator::default(), Separator::horizontal());
    }

    #[test]
    fn shorthand_matches_the_decorated_root() {
        let shorthand = separator(SeparatorOrientation::Vertical);
        let decorated = Separator::vertical().root_with(div());
        assert_eq!(shorthand.accessibility.role, decorated.accessibility.role);
        assert_eq!(
            shorthand.accessibility.orientation,
            decorated.accessibility.orientation
        );
        assert_eq!(shorthand.visual.background, None);
    }

    struct SeparatorView;

    impl SeparatorView {
        fn horizontal_id() -> ElementId {
            "rule".into()
        }

        fn vertical_id() -> ElementId {
            "spacer".into()
        }
    }

    impl View for SeparatorView {
        fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
                .child(text("Above"))
                .child(
                    Separator::horizontal()
                        .root_with(div().id(Self::horizontal_id()).h(1.0).w(120.0)),
                )
                .child(text("Below"))
                .child(
                    Separator::vertical().root_with(div().id(Self::vertical_id()).w(1.0).h(20.0)),
                )
        }
    }

    #[test]
    fn separators_project_native_dividers_and_stay_asleep() {
        let (mut cx, view) = TestAppContext::new(SeparatorView).unwrap();
        let window = view.window_handle();
        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("separator accessibility node")
        };
        let horizontal = node(SeparatorView::horizontal_id());
        assert_eq!(horizontal.role(), accesskit::Role::Splitter);
        assert_eq!(
            horizontal.orientation(),
            Some(accesskit::Orientation::Horizontal)
        );
        let vertical = node(SeparatorView::vertical_id());
        assert_eq!(vertical.role(), accesskit::Role::Splitter);
        assert_eq!(
            vertical.orientation(),
            Some(accesskit::Orientation::Vertical)
        );

        // A separator is not a Tab stop and adds no idle source.
        assert!(cx.focus(window, SeparatorView::horizontal_id()).is_err());
        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
