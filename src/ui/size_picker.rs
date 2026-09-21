//! Anchored selectors shared by the two size sheets.
use super::image_size::ImageSizing;
use super::*;
use crate::ui::dropdown::Dropdown;
use compositor::geometry::Sampling;
use quickgui::{PickerItem, SelectPopoverLayout};

#[derive(Clone, Copy)]
pub(super) enum Unit {
    Pixels,
    Percent,
    Inches,
    Centimeters,
}
impl Unit {
    pub fn value(self) -> &'static str {
        match self {
            Self::Pixels => "px",
            Self::Percent => "percent",
            Self::Inches => "inches",
            Self::Centimeters => "cm",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Pixels => "Pixels",
            Self::Percent => "Percent",
            Self::Inches => "Inches",
            Self::Centimeters => "Centimeters",
        }
    }
    fn image_sizing(self, resample: bool) -> ImageSizing {
        match (self, resample) {
            (Self::Pixels, _) => ImageSizing::Pixels,
            (Self::Percent, _) => ImageSizing::Percent,
            (Self::Inches, true) => ImageSizing::Inches,
            (Self::Centimeters, true) => ImageSizing::Centimeters,
            (Self::Inches, false) => ImageSizing::PrintInches,
            (Self::Centimeters, false) => ImageSizing::PrintCentimeters,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Fill {
    Transparent,
    Foreground,
    Background,
    Black,
    White,
    Custom,
}
impl Fill {
    fn value(self) -> &'static str {
        match self {
            Self::Transparent => "transparent",
            Self::Foreground => "foreground",
            Self::Background => "background",
            Self::Black => "black",
            Self::White => "white",
            Self::Custom => "custom",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Transparent => "Transparent",
            Self::Foreground => "Foreground",
            Self::Background => "Background",
            Self::Black => "Black",
            Self::White => "White",
            Self::Custom => "Custom",
        }
    }
}

pub(super) struct SizeMenus {
    pub units: Dropdown<Unit>,
    pub print_units: Dropdown<Unit>,
    pub sampling: Dropdown<Sampling>,
    pub fill: Dropdown<Fill>,
    pub custom_fill: [u8; 3],
}
fn state<T>(items: impl IntoIterator<Item = PickerItem<T>>) -> Dropdown<T> {
    Dropdown::new(items)
        .expect("Size-sheet options have unique static IDs")
        .with_layout(SelectPopoverLayout::new(190., 24.).trigger_height(24.))
}
impl SizeMenus {
    pub fn close(&mut self, cx: &mut EventContext) {
        self.units.close(cx);
        self.print_units.close(cx);
        self.sampling.close(cx);
        self.fill.close(cx);
    }
    pub fn new() -> Self {
        let units = |choices: &[Unit]| {
            state(
                choices
                    .iter()
                    .map(|unit| PickerItem::new(unit.label(), *unit).id(unit.value())),
            )
        };
        Self {
            custom_fill: [255; 3],
            units: units(&[Unit::Pixels, Unit::Percent, Unit::Inches, Unit::Centimeters]),
            print_units: units(&[Unit::Inches, Unit::Centimeters]),
            sampling: state([
                PickerItem::new("High quality", Sampling::High).id("high"),
                PickerItem::new("Smooth", Sampling::Smooth).id("smooth"),
                PickerItem::new("Nearest", Sampling::Nearest).id("nearest"),
            ]),
            fill: state(
                [
                    Fill::Transparent,
                    Fill::Foreground,
                    Fill::Background,
                    Fill::Black,
                    Fill::White,
                    Fill::Custom,
                ]
                .map(|fill| PickerItem::new(fill.label(), fill).id(fill.value())),
            ),
        }
    }
}
fn trigger<T>(state: &Dropdown<T>) -> Element {
    Editor::control(state.value_text().unwrap_or_else(|| "Choose…".into()))
        .w(190.)
        .flex_row()
        .items_center()
        .child(div().flex_1())
        .child(Icon::PopupChevron.element(14.))
}
impl Editor {
    pub(super) fn sync_size_menus(&mut self, form: &Form) {
        let Form::Edit { action, fields, .. } = form else {
            return;
        };
        match action {
            Action::CanvasSize => {
                self.size_menus.units.select_id(fields[4].1.as_str());
                let value = &fields[6].1;
                let custom = value.starts_with('#');
                if custom && let Ok([r, g, b, _]) = forms::parse_color(value) {
                    self.size_menus.custom_fill = [r, g, b];
                }
                self.size_menus
                    .fill
                    .select_id(if custom { "custom" } else { value });
            }
            Action::ImageSize => {
                let unit = match self.image_sizing {
                    ImageSizing::Pixels => "px",
                    ImageSizing::Percent => "percent",
                    ImageSizing::Inches | ImageSizing::PrintInches => "inches",
                    ImageSizing::Centimeters | ImageSizing::PrintCentimeters => "cm",
                };
                self.size_menus.units.select_id(unit);
                self.size_menus.print_units.select_id(unit);
                self.size_menus.sampling.select_id(fields[3].1.as_str());
            }
            _ => {}
        }
    }
    pub(super) fn size_unit_picker(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
    ) -> Element {
        let print = matches!(action, Action::ImageSize) && !self.image_sizing.resamples();
        let state = if print {
            &self.size_menus.print_units
        } else {
            &self.size_menus.units
        };
        state.element_with(
            cx,
            "size-unit",
            "Units",
            quickgui::StateAccessor::new(move |this: &mut Self| {
                if print {
                    &mut this.size_menus.print_units
                } else {
                    &mut this.size_menus.units
                }
            }),
            trigger(state),
            move |this, unit, cx| {
                if matches!(action, Action::CanvasSize) {
                    this.update_form_field(4, unit.value());
                } else {
                    let result =
                        this.change_image_sizing(unit.image_sizing(this.image_sizing.resamples()));
                    if let Some(Form::Edit { error, .. }) = &mut this.modal {
                        *error = result.err().map_or_else(String::new, |e| e.to_string());
                    }
                }
                this.changed(cx);
            },
        )
    }
    pub(super) fn size_sampling_picker(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let state = &self.size_menus.sampling;
        state.element(
            cx,
            "size-sampling",
            "Sampling",
            |this| &mut this.size_menus.sampling,
            trigger(state),
            |this, sampling, cx| {
                this.update_form_field(
                    3,
                    match sampling {
                        Sampling::High => "high",
                        Sampling::Smooth => "smooth",
                        Sampling::Nearest => "nearest",
                    },
                );
                this.changed(cx);
            },
        )
    }
    pub(super) fn size_fill_picker(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let state = &self.size_menus.fill;
        state.element(
            cx,
            "size-fill",
            "Canvas extension",
            |this| &mut this.size_menus.fill,
            trigger(state),
            |this, fill, cx| {
                if matches!(fill, Fill::Custom) {
                    let [r, g, b] = this.size_menus.custom_fill;
                    this.update_form_field(6, &format!("#{r:02X}{g:02X}{b:02X}"));
                } else {
                    this.update_form_field(6, fill.value());
                }
                this.changed(cx);
            },
        )
    }
}
