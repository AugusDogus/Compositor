use super::*;

#[derive(Clone, Copy)]
struct Tip {
    diameter: f64,
    hardness: f64,
    opacity: f64,
}

impl Tip {
    fn from_brush(brush: Brush) -> Self {
        Self {
            diameter: brush.diameter,
            hardness: brush.hardness,
            opacity: brush.opacity,
        }
    }

    fn apply(self, brush: &mut Brush) {
        brush.diameter = self.diameter;
        brush.hardness = self.hardness;
        brush.opacity = self.opacity;
    }
}

#[derive(Clone, Copy)]
enum BrushMode {
    Paint,
    Erase,
}
#[derive(Clone, Copy)]
enum Marquee {
    Rectangle,
    Ellipse,
}
#[derive(Clone, Copy)]
enum Lasso {
    Freehand,
    Polygon,
}
#[derive(Clone, Copy)]
enum Smear {
    Blur,
    Smudge,
    Liquify,
}

pub(super) struct ToolPreferences {
    tips: [Tip; 3],
    brush: BrushMode,
    marquee: Marquee,
    lasso: Lasso,
    smear: Smear,
}

impl Default for ToolPreferences {
    fn default() -> Self {
        let soft = Tip {
            diameter: 40.,
            hardness: 0.,
            opacity: 1.,
        };
        Self {
            tips: [Tip::from_brush(Brush::default()), soft, soft],
            brush: BrushMode::Paint,
            marquee: Marquee::Rectangle,
            lasso: Lasso::Freehand,
            smear: Smear::Liquify,
        }
    }
}

impl ToolPreferences {
    pub fn select(&mut self, from: Tool, to: Tool, brush: &mut Brush) {
        let family = |tool| match tool {
            Tool::Clone => 1,
            Tool::Blur | Tool::Smudge | Tool::Liquify => 2,
            _ => 0,
        };
        let (from, to_family) = (family(from), family(to));
        if from != to_family {
            self.tips[from] = Tip::from_brush(*brush);
            self.tips[to_family].apply(brush);
        }
        match to {
            Tool::Brush => self.brush = BrushMode::Paint,
            Tool::Erase => self.brush = BrushMode::Erase,
            Tool::Rectangle => self.marquee = Marquee::Rectangle,
            Tool::Ellipse => self.marquee = Marquee::Ellipse,
            Tool::Lasso => self.lasso = Lasso::Freehand,
            Tool::Polygon => self.lasso = Lasso::Polygon,
            Tool::Blur => self.smear = Smear::Blur,
            Tool::Smudge => self.smear = Smear::Smudge,
            Tool::Liquify => self.smear = Smear::Liquify,
            _ => {}
        }
    }

    pub fn brush(&self) -> Tool {
        match self.brush {
            BrushMode::Paint => Tool::Brush,
            BrushMode::Erase => Tool::Erase,
        }
    }
    pub fn marquee(&self) -> Tool {
        match self.marquee {
            Marquee::Rectangle => Tool::Rectangle,
            Marquee::Ellipse => Tool::Ellipse,
        }
    }
    pub fn lasso(&self) -> Tool {
        match self.lasso {
            Lasso::Freehand => Tool::Lasso,
            Lasso::Polygon => Tool::Polygon,
        }
    }
    pub fn smear(&self) -> Tool {
        match self.smear {
            Smear::Blur => Tool::Blur,
            Smear::Smudge => Tool::Smudge,
            Smear::Liquify => Tool::Liquify,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};
    #[test]
    fn brush_rail_recalls_erase_while_shortcuts_choose_the_explicit_mode() {
        let editor = Editor::with_test_document();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Brush mode").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.simulate_keystrokes(window, "e v").unwrap();
        cx.click(window, "brush-tool").unwrap();
        assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Erase);
        cx.simulate_keystrokes(window, "b v").unwrap();
        cx.click(window, "brush-tool").unwrap();
        assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Brush);
        cx.simulate_keystrokes(window, "e").unwrap();
        assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Erase);
    }

    #[test]
    fn brush_families_keep_independent_tips_and_shortcuts_recall_selected_modes() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
        let (mut cx, editor) = Application::new()
            .into_test_context(
                WindowOptions::new("Tool preferences").size(1280., 900.),
                editor,
            )
            .unwrap();
        cx.update(editor, |e, cx| {
            e.tools.brush.diameter = 90.;
            e.tools.brush.hardness = 0.7;
            e.tools.brush.opacity = 0.4;
            e.tools.brush.color = [255, 0, 0, 255];
            e.select_tool(Tool::Clone, cx);
            assert_eq!(
                (
                    e.tools.brush.diameter,
                    e.tools.brush.hardness,
                    e.tools.brush.opacity
                ),
                (40., 0., 1.)
            );
            e.tools.brush.diameter = 25.;
            e.select_tool(Tool::Liquify, cx);
            assert_eq!(e.tools.brush.diameter, 40.);
            e.tools.brush.diameter = 60.;
            e.select_tool(Tool::Blur, cx);
            assert_eq!(e.tools.brush.diameter, 60.);
            e.select_tool(Tool::Brush, cx);
            assert_eq!(
                (
                    e.tools.brush.diameter,
                    e.tools.brush.hardness,
                    e.tools.brush.opacity
                ),
                (90., 0.7, 0.4)
            );
            assert_eq!(e.tools.brush.color, [255, 0, 0, 255]);
            e.select_tool(Tool::Clone, cx);
            assert_eq!(e.tools.brush.diameter, 25.);
            e.select_tool(Tool::Ellipse, cx);
            e.select_tool(Tool::Polygon, cx);
            e.key(&Key::Character("m".into()), Modifiers::empty(), cx);
            assert_eq!(e.tools.tool, Tool::Ellipse);
            e.key(&Key::Character("l".into()), Modifiers::empty(), cx);
            assert_eq!(e.tools.tool, Tool::Polygon);
            e.key(&Key::Character("r".into()), Modifiers::empty(), cx);
            assert_eq!(e.tools.tool, Tool::Blur);
            assert_eq!(e.tools.brush.diameter, 60.);
        })
        .unwrap();
    }
}
