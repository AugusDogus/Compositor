use crate::{
    Result,
    adjustment::Kind,
    document::{Document, LayerContent},
    geometry::{Point, Transform},
    guides::Axis,
};

#[derive(Clone, Copy, Debug)]
pub enum QuarterTurn {
    Clockwise,
    CounterClockwise,
}

impl QuarterTurn {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clockwise => "Rotate Canvas 90° Clockwise",
            Self::CounterClockwise => "Rotate Canvas 90° Counterclockwise",
        }
    }

    fn angle(self) -> f64 {
        match self {
            Self::Clockwise => 90.,
            Self::CounterClockwise => -90.,
        }
    }

    fn point(self, [x, y]: Point, [width, height]: Point) -> Point {
        match self {
            Self::Clockwise => [height - y, x],
            Self::CounterClockwise => [y, width - x],
        }
    }

    fn transform(self, transform: &mut Transform, size: Point) {
        let center = self.point(transform.point([0.5, 0.5]), size);
        transform.origin = [
            center[0] - transform.size[0] / 2.,
            center[1] - transform.size[1] / 2.,
        ];
        transform.rotation += self.angle();
    }
}

/// Rotate placements rather than resampling layers; validate before replacing the document.
pub fn rotate(document: &mut Document, turn: QuarterTurn) -> Result<()> {
    let size = [f64::from(document.width), f64::from(document.height)];
    let mut rotated = document.clone();
    std::mem::swap(&mut rotated.width, &mut rotated.height);
    for layer in &mut rotated.layers {
        turn.transform(&mut layer.transform, size);
        if let Some(placement) = layer.mask.as_mut().and_then(|mask| mask.placement.as_mut()) {
            turn.transform(placement, size);
        }
        if let LayerContent::Adjustment(adjustment) = &mut layer.content
            && adjustment.kind == Kind::MotionBlur
        {
            adjustment.motion_angle = Some(
                (adjustment.motion_angle.unwrap_or(0.) + turn.angle() + 90.).rem_euclid(180.) - 90.,
            );
        }
    }
    for guide in &mut rotated.guides {
        let point = match guide.axis {
            Axis::Horizontal => [0., guide.position],
            Axis::Vertical => [guide.position, 0.],
        };
        guide.axis = match guide.axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };
        guide.position = turn.point(point, size)[guide.axis.index()];
    }
    if let Some(selection) = &document.selection
        && let Some(bounds) = selection.bounds()
    {
        let inverse = match turn {
            QuarterTurn::Clockwise => QuarterTurn::CounterClockwise,
            QuarterTurn::CounterClockwise => QuarterTurn::Clockwise,
        };
        let a = turn.point([bounds[0], bounds[1]], size);
        let b = turn.point([bounds[2], bounds[3]], size);
        rotated.selection = Some(selection.mapped(
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[0].max(b[0]),
                a[1].max(b[1]),
            ],
            |p| turn.point(p, size),
            |p| inverse.point(p, [size[1], size[0]]),
        )?);
    }
    rotated.validate()?;
    *document = rotated;
    Ok(())
}
