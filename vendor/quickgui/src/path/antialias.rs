use std::{mem, sync::Arc};

pub(crate) struct StrokeAntialias {
    width: f32,
    indices: Arc<[u32]>,
    boundary_normals: Arc<[BoundaryNormals]>,
    stroke_normals: Arc<[[f32; 2]]>,
    centerline: Arc<[[f32; 2]]>,
}

impl StrokeAntialias {
    pub(super) fn new(
        width: f32,
        indices: Vec<u32>,
        boundary_normals: Vec<BoundaryNormals>,
        stroke_normals: Vec<[f32; 2]>,
        centerline: Vec<[f32; 2]>,
    ) -> Self {
        Self {
            width,
            indices: indices.into(),
            boundary_normals: boundary_normals.into(),
            stroke_normals: stroke_normals.into(),
            centerline: centerline.into(),
        }
    }

    pub(super) fn byte_len(&self) -> usize {
        self.indices.len() * mem::size_of::<u32>()
            + self.boundary_normals.len() * mem::size_of::<BoundaryNormals>()
            + self.stroke_normals.len() * mem::size_of::<[f32; 2]>()
            + self.centerline.len() * mem::size_of::<[f32; 2]>()
    }

    pub(crate) fn offset(&self, vertex: usize, transform: [f32; 2], scale: f32) -> [f32; 2] {
        self.boundary_normals[self.indices[vertex] as usize].offset(transform, scale)
    }

    pub(crate) fn centerline(&self, vertex: usize) -> [f32; 2] {
        self.centerline[self.indices[vertex] as usize]
    }

    pub(crate) fn width_pixels(&self, vertex: usize, transform: [f32; 2], scale: f32) -> f32 {
        let [x, y] = self.stroke_normals[self.indices[vertex] as usize];
        let denominator = (x / transform[0]).hypot(y / transform[1]);
        if denominator > 0. && denominator.is_finite() {
            self.width * x.hypot(y) / denominator * scale
        } else {
            self.width * transform[0].abs().min(transform[1].abs()) * scale
        }
    }
}

/// Outward normals of the mesh boundary at a vertex. Interior vertices stay fixed.
/// Junctions cannot be expanded as a single corner without changing mesh topology.
#[derive(Clone, Copy, Default)]
pub(super) enum BoundaryNormals {
    #[default]
    Interior,
    Edge([f32; 2]),
    Corner([f32; 2], [f32; 2]),
    Junction,
}

impl BoundaryNormals {
    pub(super) fn add(&mut self, normal: [f32; 2]) {
        *self = match *self {
            Self::Interior => Self::Edge(normal),
            Self::Edge(first) => Self::Corner(first, normal),
            Self::Corner(..) | Self::Junction => Self::Junction,
        };
    }

    pub(super) fn offset(self, transform: [f32; 2], display_scale: f32) -> [f32; 2] {
        let (first, second) = match self {
            Self::Interior | Self::Junction => return [0.; 2],
            Self::Edge(normal) => (normal, normal),
            Self::Corner(first, second) => (first, second),
        };
        // Inverse-transpose normals preserve the fringe through nonuniform scaling
        // and reflections. Expansion is in display pixels, not path coordinates.
        let normalize = |normal: [f32; 2]| {
            let x = f64::from(normal[0]) / f64::from(transform[0]);
            let y = f64::from(normal[1]) / f64::from(transform[1]);
            let length = x.hypot(y);
            (length.is_finite() && length > 0.).then(|| [x / length, y / length])
        };
        let (Some(first), Some(second)) = (normalize(first), normalize(second)) else {
            return [0.; 2];
        };
        let sum = [first[0] + second[0], first[1] + second[1]];
        let length = sum[0].hypot(sum[1]);
        if length < 1e-6 {
            return [0.; 2];
        }
        let direction = [sum[0] / length, sum[1] / length];
        let projection = direction[0] * first[0] + direction[1] * first[1];
        // A bounded miter is sufficient for the half-pixel coverage ramp. Cap
        // low-DPI expansion at one logical pixel so retained paint bounds remain
        // independent of the target scale.
        let amount = 1. / (projection.max(0.25) * f64::from(display_scale.max(1.)));
        [
            (direction[0] * amount / f64::from(transform[0])) as f32,
            (direction[1] * amount / f64::from(transform[1])) as f32,
        ]
    }
}
