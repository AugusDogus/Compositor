use super::{canvas::combine, *};
use compositor::{
    geometry::Point,
    selection::{Selection, SelectionMode},
};

/// The first click fixes the mode until this outline is committed or cancelled.
pub(super) struct PolygonDraft {
    pub points: Vec<Point>,
    pub mode: SelectionMode,
}

/// Swift distinguishes an unfinished outline from a valid outline clipped outside the canvas.
pub(super) fn lasso_is_degenerate(points: &[Point]) -> bool {
    points.len() < 3
        || points.first().is_none_or(|first| {
            points.iter().all(|point| point[0] == first[0])
                || points.iter().all(|point| point[1] == first[1])
        })
}

pub(super) fn is_deselect_gesture(gesture: &Gesture) -> bool {
    match gesture {
        Gesture::Region {
            tool: Tool::Rectangle | Tool::Ellipse,
            start,
            end,
            mode: SelectionMode::Replace,
            ..
        } => start[0] == end[0] || start[1] == end[1],
        Gesture::Lasso {
            points,
            mode: SelectionMode::Replace,
            ..
        } => lasso_is_degenerate(points),
        _ => false,
    }
}

impl Action {
    pub(super) fn edits_selection(self) -> bool {
        matches!(
            self,
            Self::SelectSubject
                | Self::SelectAll
                | Self::Deselect
                | Self::InvertSelection
                | Self::LoadAlpha
                | Self::LoadMask
        )
    }
}

impl Editor {
    pub(super) fn finish_selection_draft(&mut self, gesture: &Gesture) -> Result<()> {
        let doc = &self.session().document;
        let (next, base, mode) = match gesture {
            Gesture::Region {
                start,
                end,
                tool,
                base,
                mode,
                ..
            } if start[0] != end[0] && start[1] != end[1] => (
                Selection::marquee(
                    doc.width,
                    doc.height,
                    *start,
                    *end,
                    *tool == Tool::Ellipse,
                    self.tools.selection_antialiased,
                )?,
                base,
                *mode,
            ),
            Gesture::Lasso { points, base, mode } if !lasso_is_degenerate(points) => (
                Selection::polygon(
                    doc.width,
                    doc.height,
                    points,
                    self.tools.selection_antialiased,
                )?,
                base,
                *mode,
            ),
            _ => return Ok(()),
        };
        self.session_mut().document.selection = combine(base, next, mode)?;
        Ok(())
    }

    pub(super) fn can_modify_selection(&self) -> bool {
        self.can_edit_layers()
            && self.tools.polygon.is_none()
            && self
                .session()
                .document
                .selection
                .as_ref()
                .is_some_and(|s| s.bounds().is_some())
    }
    pub(super) fn feather_selection(&mut self, amount: u16) -> Result<()> {
        if !self.can_modify_selection() {
            return Err(compositor::invalid(
                "Finish the current edit and draw a nonempty selection before feathering.",
            ));
        }
        self.apply_selection_feather(amount)
    }

    pub(super) fn apply_selection_feather(&mut self, amount: u16) -> Result<()> {
        self.session_mut().edit("Feather Selection", |doc| {
            let selection = doc
                .selection
                .as_ref()
                .ok_or_else(|| compositor::invalid("Draw a selection before feathering."))?;
            doc.selection = Some(selection.feathered(amount, doc.width, doc.height)?);
            Ok(())
        })?;
        self.tools.selection_feather_amount = amount;
        Ok(())
    }

    pub(super) fn resize_selection(&mut self, expand: bool, amount: u16) -> Result<()> {
        if !(1..=500).contains(&amount) {
            return Err(compositor::invalid(
                "Enter a whole number from 1 to 500 pixels.",
            ));
        }
        if !self.can_modify_selection() {
            return Err(compositor::invalid(
                "Finish or cancel the current edit, then draw a nonempty selection before resizing it.",
            ));
        }
        self.session_mut().edit(
            if expand {
                "Expand Selection"
            } else {
                "Contract Selection"
            },
            |doc| {
                let selection = doc
                    .selection
                    .as_ref()
                    .filter(|s| s.bounds().is_some())
                    .ok_or_else(|| {
                        compositor::invalid(
                            "Draw a nonempty selection before expanding or contracting it.",
                        )
                    })?;
                doc.selection = Some(selection.resized(
                    if expand {
                        i32::from(amount)
                    } else {
                        -(i32::from(amount))
                    },
                    doc.width,
                    doc.height,
                )?);
                Ok(())
            },
        )?;
        if expand {
            self.tools.selection_expand_amount = amount;
        } else {
            self.tools.selection_contract_amount = amount;
        }
        Ok(())
    }

    pub(super) fn polygon_click(
        &mut self,
        point: Point,
        zoom: f64,
        mode: SelectionMode,
    ) -> Result<()> {
        if let Some(draft) = &mut self.tools.polygon {
            if draft.points.len() >= 3
                && draft.points.first().is_some_and(|first| {
                    (point[0] - first[0]).hypot(point[1] - first[1]) * zoom <= 8.
                })
            {
                return self.commit_polygon();
            }
            if draft
                .points
                .last()
                .is_none_or(|last| (point[0] - last[0]).hypot(point[1] - last[1]) >= 0.25)
            {
                draft.points.push(point);
            }
        } else {
            self.tools.polygon = Some(PolygonDraft {
                points: vec![point],
                mode,
            });
        }
        self.status = "Click vertices or the first vertex to close. Enter applies, Backspace removes a vertex, Escape cancels.".into();
        Ok(())
    }

    pub(super) fn commit_polygon(&mut self) -> Result<()> {
        let Some(draft) = self.tools.polygon.take() else {
            return Ok(());
        };
        let antialiased = self.tools.selection_antialiased;
        let degenerate = lasso_is_degenerate(&draft.points);
        let result = self.session_mut().edit(
            if degenerate {
                "Deselect"
            } else {
                "Polygonal Lasso"
            },
            |doc| {
                let next = Selection::polygon(doc.width, doc.height, &draft.points, antialiased)?;
                if degenerate {
                    if draft.mode == SelectionMode::Replace {
                        doc.selection = None;
                    }
                } else {
                    doc.selection = combine(&doc.selection, next, draft.mode)?;
                }
                Ok(())
            },
        );
        if result.is_err() {
            self.tools.polygon = Some(draft);
        }
        result
    }

    pub(super) fn finish_polygon(&mut self, cx: &mut EventContext) {
        let result = self.commit_polygon();
        self.operation_result(alerts::Operation::Paint, result, cx);
    }
}
