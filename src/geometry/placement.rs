use super::{
    Point, Transform,
    projective::{Error, Homography, Projective},
};

impl Transform {
    pub fn affine_mapping(self) -> Result<Homography, Error> {
        let affine = Self { warp: None, ..self };
        let a = affine.point([0., 0.]);
        let b = affine.point([1., 0.]);
        let c = affine.point([0., 1.]);
        Homography::from_matrix([
            b[0] - a[0],
            c[0] - a[0],
            a[0],
            b[1] - a[1],
            c[1] - a[1],
            a[1],
            0.,
            0.,
            1.,
        ])
    }

    fn flips(self) -> Result<Homography, Error> {
        Homography::from_matrix([
            if self.flip_x { -1. } else { 1. },
            0.,
            f64::from(self.flip_x),
            0.,
            if self.flip_y { -1. } else { 1. },
            f64::from(self.flip_y),
            0.,
            0.,
            1.,
        ])
    }

    pub fn geometry_mapping(self) -> Result<Homography, Error> {
        let affine = Self {
            flip_x: false,
            flip_y: false,
            warp: None,
            ..self
        }
        .affine_mapping()?;
        match self.warp {
            Some(warp) => affine.compose(warp.mapping()),
            None => Ok(affine),
        }
    }

    pub fn mapping(self) -> Result<Homography, Error> {
        self.geometry_mapping()?.compose(self.flips()?)
    }

    pub fn inverse_mapping(self) -> Result<Homography, Error> {
        self.mapping()?.inverse()
    }

    pub fn try_point(self, point: Point) -> Result<Point, Error> {
        self.mapping()?.apply(point)
    }

    pub fn try_unit(self, point: Point) -> Result<Point, Error> {
        self.inverse_mapping()?.apply(point)
    }

    /// Keep the affine controls while representing the full exact placement.
    pub fn with_mapping(mut self, mapping: Homography) -> crate::Result<Self> {
        let warp = Self {
            flip_x: false,
            flip_y: false,
            warp: None,
            ..self
        }
        .affine_mapping()
        .and_then(|a| a.inverse())
        .and_then(|inverse| inverse.compose(mapping))
        .and_then(|mapping| mapping.compose(self.flips()?))
        .and_then(Projective::from_mapping)
        .map_err(|error| crate::invalid(error.to_string()))?;
        self.warp = Some(warp);
        if !self.valid() {
            return Err(crate::invalid(
                "Perspective exceeds the supported layer bounds. Reduce the distortion; the layer is unchanged.",
            ));
        }
        Ok(self)
    }

    /// Move this placement with a changed selection without discarding shear or
    /// perspective. Callers prepare all linked placements before changing layers.
    pub fn following(self, old: Self, new: Self) -> crate::Result<Self> {
        if !old.valid() || !new.valid() || !self.valid() {
            return Err(crate::invalid("The transform exceeds supported bounds."));
        }
        if self == old {
            return Ok(new);
        }
        if old == new {
            return Ok(self);
        }
        if (Self {
            origin: new.origin,
            ..old
        }) == new
        {
            let translated = Self {
                origin: [
                    self.origin[0] + (new.origin[0] - old.origin[0]),
                    self.origin[1] + (new.origin[1] - old.origin[1]),
                ],
                ..self
            };
            return if translated.valid() {
                Ok(translated)
            } else {
                Err(crate::invalid(
                    "Moving would exceed the supported layer bounds. The document is unchanged.",
                ))
            };
        }
        let mapping = new
            .mapping()
            .and_then(|next| next.compose(old.inverse_mapping()?))
            .and_then(|change| change.compose(self.mapping()?))
            .map_err(|error| crate::invalid(error.to_string()))?;
        let corners = mapping
            .map_rectangle([0., 0., 1., 1.])
            .map_err(|error| crate::invalid(error.to_string()))?;
        let [a, b, _, c] = corners;
        let center = mapping
            .apply([0.5, 0.5])
            .map_err(|error| crate::invalid(error.to_string()))?;
        let sign = if self.flip_x { -1. } else { 1. };
        let angle = ((b[1] - a[1]) * sign).atan2((b[0] - a[0]) * sign);
        let along = -(c[0] - a[0]) * angle.sin() + (c[1] - a[1]) * angle.cos();
        let size = [
            (b[0] - a[0]).hypot(b[1] - a[1]).max(1.),
            along.abs().max(1.),
        ];
        let degrees = angle.to_degrees();
        let candidate = Self {
            origin: [center[0] - size[0] / 2., center[1] - size[1] / 2.],
            size,
            rotation: degrees + ((self.rotation - degrees) / 360.).round() * 360.,
            flip_y: along < 0.,
            warp: None,
            ..self
        };
        let affine = self.warp.is_none() && old.warp.is_none() && new.warp.is_none();
        if affine
            && candidate.valid()
            && [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
                .into_iter()
                .zip(corners)
                .all(|(unit, expected)| {
                    candidate
                        .point(unit)
                        .into_iter()
                        .zip(expected)
                        .all(|(a, b)| (a - b).abs() <= 1e-9)
                })
        {
            return Ok(candidate);
        }
        candidate.with_mapping(mapping)
    }

    /// Rebind the source grid after crop/padding, preserving all old document
    /// coordinates. The full expanded domain must stay on one side of infinity.
    pub fn rebind(self, origin: Point, size: Point) -> crate::Result<Self> {
        if size.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err(crate::invalid(
                "The new source dimensions must be finite and positive.",
            ));
        }
        let bounds = [
            origin[0],
            origin[1],
            origin[0] + size[0],
            origin[1] + size[1],
        ];
        let mapping = self.mapping().map_err(|e| crate::invalid(e.to_string()))?;
        mapping
            .map_rectangle(bounds)
            .map_err(|e| crate::invalid(e.to_string()))?;
        let domain =
            Homography::from_matrix([size[0], 0., origin[0], 0., size[1], origin[1], 0., 0., 1.])
                .map_err(|e| crate::invalid(e.to_string()))?;
        let center = self
            .try_point([origin[0] + size[0] * 0.5, origin[1] + size[1] * 0.5])
            .map_err(|e| crate::invalid(e.to_string()))?;
        let dimensions = [self.size[0] * size[0], self.size[1] * size[1]];
        let candidate = Self {
            origin: [
                center[0] - dimensions[0] * 0.5,
                center[1] - dimensions[1] * 0.5,
            ],
            size: dimensions,
            warp: None,
            ..self
        };
        if self.warp.is_none() && candidate.valid() {
            return Ok(candidate);
        }
        candidate.with_mapping(
            mapping
                .compose(domain)
                .map_err(|e| crate::invalid(e.to_string()))?,
        )
    }
}
