use crate::{
    Result,
    geometry::{
        Transform,
        projective::{Error, Homography},
    },
    invalid,
};

pub(super) fn pixel_mapping(transform: Transform, size: [u32; 2]) -> Result<Homography> {
    let domain = Homography::from_matrix([
        1. / f64::from(size[0]),
        0.,
        0.5 / f64::from(size[0]),
        0.,
        1. / f64::from(size[1]),
        0.5 / f64::from(size[1]),
        0.,
        0.,
        1.,
    ])
    .map_err(|e| invalid(e.to_string()))?;
    transform
        .mapping()
        .and_then(|mapping| mapping.compose(domain))
        .map_err(|e| invalid(e.to_string()))
}

pub(super) fn antialias_bound(transform: Transform, size: [u32; 2]) -> Result<f64> {
    if transform.warp.is_none() {
        return Ok((transform.size[0] / f64::from(size[0]))
            .min(transform.size[1] / f64::from(size[1]))
            .max(0.001));
    }
    let scale = transform
        .mapping()
        .and_then(|mapping| mapping.derivative_bounds([0., 0., 1., 1.]))
        .map_err(|e| invalid(e.to_string()))?;
    Ok((scale[0] / f64::from(size[0]))
        .min(scale[1] / f64::from(size[1]))
        .max(0.001))
}

pub(super) fn source_bounds(
    transform: Transform,
    size: [u32; 2],
    bounds: [f64; 4],
) -> Result<[u32; 4]> {
    let mapping = transform
        .inverse_mapping()
        .map_err(|e| invalid(e.to_string()))?;
    let corners = match mapping.map_rectangle(bounds) {
        Ok(corners) => corners,
        // The preimage may be unbounded. Visiting the existing finite source
        // is conservative and correct for erasing or selection-limited strokes.
        Err(Error::Horizon) => return Ok([0, 0, size[0], size[1]]),
        Err(error) => return Err(invalid(error.to_string())),
    };
    let min = [0, 1].map(|axis| {
        corners
            .iter()
            .map(|p| p[axis] * f64::from(size[axis]))
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.) as u32
    });
    let max = [0, 1].map(|axis| {
        corners
            .iter()
            .map(|p| p[axis] * f64::from(size[axis]))
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .max(0.) as u32
    });
    Ok([
        min[0].min(size[0]),
        min[1].min(size[1]),
        max[0].min(size[0]),
        max[1].min(size[1]),
    ])
}
