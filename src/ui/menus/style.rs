//! Shared AppKit-style surfaces and rows for application and picker menus.
use super::*;

pub(in crate::ui) fn surface() -> Element {
    div()
        .bg(Color::rgba8(64, 64, 64, 217))
        .backdrop_blur(20.)
        .border(1., Color::rgb8(87, 87, 87))
        .rounded(12.)
        .shadow(crate::ui::surfaces::menu_shadow())
}

pub(in crate::ui) fn choice(
    label: impl Into<Arc<str>>,
    state: quickgui::PopoverMenuItemState,
) -> Element {
    let mark = if state.checked == Some(true) {
        Icon::Check.element(9.).absolute().left(-2.)
    } else {
        div().absolute()
    };
    div()
        .h(24.)
        .flex_shrink_0()
        .px(9.)
        .flex_row()
        .items_center()
        .gap(24.)
        .rounded(5.)
        .bg(if state.highlighted && !state.disabled {
            Color::rgb8(0, 106, 216)
        } else {
            Color::TRANSPARENT
        })
        .text_color(if state.disabled {
            Color::rgb8(117, 117, 117)
        } else {
            Color::rgb8(231, 231, 231)
        })
        .child(mark)
        .child(
            text(label)
                .text_size(13.)
                .flex_grow(1.)
                .flex_shrink_0()
                .whitespace_nowrap(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    struct MaterialSample {
        dark: bool,
    }
    impl View for MaterialSample {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let backdrop = quickgui::canvas(|_, painter| {
                for x in (0..256).step_by(4) {
                    painter.fill_rect(quickgui::Rect::new(x as f32, 0., 2., 96.), Color::BLACK);
                }
            })
            .absolute()
            .size_full();
            let mut root = div()
                .size_full()
                .bg(if self.dark {
                    Color::BLACK
                } else {
                    Color::WHITE
                })
                .font_family("Inter Variable")
                .child(backdrop);
            for x in [20., 160.] {
                root = root.child(surface().absolute().left(x).top(16.).w(88.).h(64.).child(
                    choice(
                        "Menu",
                        quickgui::PopoverMenuItemState {
                            highlighted: false,
                            disabled: false,
                            checked: None,
                            has_submenu: false,
                        },
                    ),
                ));
            }
            root
        }
    }

    #[test]
    fn menu_material_blurs_the_backdrop_but_keeps_text_and_outside_pixels_sharp() {
        for (width, height) in [(256, 96), (1920, 1080)] {
            check_material(width, height);
        }
    }

    // Headless captures use 2x scale, so the large case exercises a 4K workspace
    // with both a parent menu and a submenu material visible.
    fn check_material(width: u32, height: u32) {
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Menu material").size(width as f32, height as f32),
                MaterialSample { dark: false },
            )
            .unwrap();
        let frame = cx.capture_screenshot(view.window_handle()).unwrap();
        let pixel = |x, y| {
            frame
                .pixel(x * frame.width() / width, y * frame.height() / height)
                .unwrap()
        };
        assert_eq!(pixel(4, 4), [0, 0, 0, 255]);
        assert_eq!(pixel(6, 4), [255, 255, 255, 255]);
        for left in [36, 176] {
            let values: Vec<_> = (left..left + 56).map(|x| pixel(x, 62)[0]).collect();
            let low = *values.iter().min().unwrap();
            let high = *values.iter().max().unwrap();
            assert!(
                high - low < 8,
                "Both menus must blur their backdrop at {width}x{height}: {low}..{high}"
            );
        }
        assert!(
            (20..40).any(|y| (28..94).any(|x| pixel(x, y)[0] > 200)),
            "Menu lettering must remain sharp above the blurred backdrop"
        );
        // Keep menu content unchanged while replacing the background. A cached menu must
        // still recapture what is behind it, rather than turn into a static opaque panel.
        cx.update(view, |sample, cx| {
            sample.dark = true;
            cx.invalidate();
        })
        .unwrap();
        let dark = cx.capture_screenshot(view.window_handle()).unwrap();
        let dark_value = dark
            .pixel(64 * dark.width() / width, 62 * dark.height() / height)
            .unwrap()[0];
        assert!(
            pixel(64, 62)[0] > dark_value + 8,
            "The material must update with its backdrop: {} over stripes, {dark_value} over black",
            pixel(64, 62)[0]
        );
    }
}
