use super::*;
use compositor::{
    floating::FloatingPixels,
    geometry::{Point, Transform},
    transform::{self, Handle},
};

#[derive(Clone, Copy)]
pub(super) enum Placement {
    Affine(Transform),
    Perspective([Point; 4]),
}

impl Placement {
    pub fn corners(self) -> [Point; 4] {
        match self {
            Self::Affine(t) => {
                [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| t.geometry_point(p))
            }
            Self::Perspective(c) => c,
        }
    }

    pub fn bounds(self) -> Transform {
        match self {
            Self::Affine(t) => t,
            Self::Perspective(c) => {
                let left = c.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                let top = c.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                let right = c.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
                let bottom = c.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
                Transform {
                    origin: [left, top],
                    size: [right - left, bottom - top],
                    ..Transform::new(1, 1)
                }
            }
        }
    }

    pub fn handles(self) -> [Point; 9] {
        let c = self.corners();
        let midpoint = |a: Point, b: Point| [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.];
        [
            c[0],
            midpoint(c[0], c[1]),
            c[1],
            midpoint(c[1], c[2]),
            c[2],
            midpoint(c[2], c[3]),
            c[3],
            midpoint(c[3], c[0]),
            self.bounds().geometry_point([0.5, 0.]),
        ]
    }

    pub fn following(self, new: Transform) -> Self {
        match self {
            Self::Affine(_) => Self::Affine(new),
            Self::Perspective(c) => {
                let old = self.bounds();
                Self::Perspective(c.map(|p| new.point(old.unit(p))))
            }
        }
    }
}

pub(super) struct PendingPixels {
    source: FloatingPixels,
    pub placement: Placement,
}

impl PendingPixels {
    pub(super) fn preview(&self, placement: Placement) -> Result<Document> {
        match placement {
            Placement::Affine(t) => self.source.preview(t, false),
            Placement::Perspective(c) => self.source.preview_distorted(c, false),
        }
    }

    pub fn pixel_size(&self) -> Point {
        self.source.placement.size
    }
}

pub(super) enum DragKind {
    Move,
    Transform(Handle),
    Distort(usize),
}

pub(super) struct PixelDrag {
    pub start: Point,
    pub original: Placement,
    pub kind: DragKind,
}

impl Editor {
    pub(super) fn can_float_selection(&self) -> bool {
        self.can_edit_pixels()
            && !self.tools.mask_target
            && self
                .session()
                .document
                .selection
                .as_ref()
                .is_some_and(|selection| selection.bounds().is_some())
            && self
                .session()
                .document
                .active_layer()
                .is_some_and(|layer| layer.raster().is_some())
    }

    pub(super) fn begin_pixel_transform(&mut self) -> Result<()> {
        let source = FloatingPixels::lift(&self.session().document)?;
        self.session_mut().begin("Transform Selection")?;
        self.pending_pixels = Some(PendingPixels {
            placement: Placement::Affine(source.placement),
            source,
        });
        self.tools.tool = Tool::Move;
        self.status = "Transform selected pixels. Ctrl-drag handles to distort. Enter applies, Escape cancels.".into();
        Ok(())
    }

    pub(super) fn preview_pixels(&mut self, placement: Placement) -> Result<()> {
        let Some(edit) = &self.pending_pixels else {
            return Ok(());
        };
        let doc = edit.preview(placement)?;
        self.session_mut().document = doc;
        if let Some(edit) = &mut self.pending_pixels {
            edit.placement = placement;
        }
        Ok(())
    }

    pub(super) fn commit_pixels(&mut self) -> Result<()> {
        if self.pending_pixels.take().is_some() {
            self.session_mut().commit()?;
        }
        Ok(())
    }

    pub(super) fn pixel_drag(
        &self,
        point: Point,
        zoom: f64,
        modifiers: Modifiers,
    ) -> Option<PixelDrag> {
        let original = self.pending_pixels.as_ref()?.placement;
        Some(PixelDrag::new(original, point, zoom, modifiers, true))
    }

    pub(super) fn drag_pixels(
        &mut self,
        drag: &PixelDrag,
        point: Point,
        modifiers: Modifiers,
    ) -> Result<()> {
        let placement = drag.placement(point, modifiers, self.tools.transform_ratio);
        if let Placement::Perspective(corners) = placement
            && !compositor::distort::usable_corners(corners)
        {
            return Ok(());
        }
        self.preview_pixels(placement)
    }
}

impl PixelDrag {
    pub(super) fn new(
        original: Placement,
        point: Point,
        zoom: f64,
        modifiers: Modifiers,
        show_controls: bool,
    ) -> Self {
        let handles = original.handles();
        let rotation = match original {
            Placement::Affine(bounds) => {
                Some(bounds.geometry_point([0.5, -28. / zoom / bounds.size[1]]))
            }
            Placement::Perspective(_) => None,
        };
        let hit = compositor::transform::hit_handle(
            std::array::from_fn(|index| handles[index]),
            rotation,
            point,
            zoom,
        )
        .filter(|_| show_controls);
        let kind = match hit {
            Some(Handle::Resize(i))
                if modifiers.contains(Modifiers::CONTROL)
                    || matches!(original, Placement::Perspective(_)) =>
            {
                DragKind::Distort(i)
            }
            Some(handle) => DragKind::Transform(handle),
            None => DragKind::Move,
        };
        Self {
            start: point,
            original,
            kind,
        }
    }

    pub(super) fn placement(
        &self,
        point: Point,
        modifiers: Modifiers,
        lock_ratio: bool,
    ) -> Placement {
        let drag = self;
        let shift = modifiers.contains(Modifiers::SHIFT);
        let mut delta = [point[0] - drag.start[0], point[1] - drag.start[1]];
        if shift {
            if delta[0].abs() >= delta[1].abs() {
                delta[1] = 0.;
            } else {
                delta[0] = 0.;
            }
        }
        let placement = match drag.kind {
            DragKind::Move => {
                let mut new = drag.original.bounds();
                new.origin[0] += delta[0].round();
                new.origin[1] += delta[1].round();
                drag.original.following(new)
            }
            DragKind::Transform(handle) => drag.original.following(transform::drag(
                drag.original.bounds(),
                drag.start,
                point,
                handle,
                lock_ratio,
                shift,
                modifiers.contains(Modifiers::ALT),
            )),
            DragKind::Distort(handle) => {
                let mut c = drag.original.corners();
                let first = handle / 2;
                for (i, corner) in c.iter_mut().enumerate() {
                    if i == first || (handle % 2 == 1 && i == (first + 1) % 4) {
                        corner[0] += delta[0];
                        corner[1] += delta[1];
                    }
                }
                Placement::Perspective(c)
            }
        };
        match placement {
            Placement::Affine(t) => Placement::Affine(Transform {
                origin: t.origin.map(f64::round),
                size: t.size.map(|v| v.round().max(1.)),
                rotation: t.rotation.round(),
                ..t
            }),
            perspective => perspective,
        }
    }
}
