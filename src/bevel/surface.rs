//! Float height fields retain subtle relief that would be lost in an 8-bit mask.
use super::{Lighting, Settings, Shading};
use crate::{Result, effects::cpu::gaussian, invalid};

pub(crate) struct Surface {
    heights: Vec<f32>,
    width: usize,
    height: usize,
    lighting: Lighting,
}

impl Surface {
    pub(crate) fn new(
        alpha: &[f32],
        width: usize,
        height: usize,
        settings: &Settings,
    ) -> Result<Self> {
        let lighting = Lighting::new(settings)?;
        if width == 0 || height == 0 || width.checked_mul(height) != Some(alpha.len()) {
            return Err(invalid(
                "The Bevel/Emboss height field does not match its pixel dimensions.",
            ));
        }
        // At this scale off-center Gaussian weights are zero in f32. Avoid
        // squaring subnormal sigma values in the general blur kernel.
        let heights = if settings.size <= 0.02 {
            alpha.to_vec()
        } else {
            gaussian(alpha, width, height, (settings.size / 2.) as f32)
        };
        Ok(Self {
            heights,
            width,
            height,
            lighting,
        })
    }

    /// Coordinates come from enumerating the validated source surface.
    pub(crate) fn sample(&self, x: usize, y: usize) -> Shading {
        let left = y * self.width + x.saturating_sub(1);
        let right = y * self.width + (x + 1).min(self.width - 1);
        let top = y.saturating_sub(1) * self.width + x;
        let bottom = (y + 1).min(self.height - 1) * self.width + x;
        self.lighting.sample([
            self.heights[right] - self.heights[left],
            self.heights[bottom] - self.heights[top],
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_field_rejects_empty_or_mismatched_dimensions() {
        for (alpha, width, height) in [(&[][..], 0, 2), (&[1.][..], 2, 2)] {
            assert!(Surface::new(alpha, width, height, &Settings::default()).is_err());
        }
    }

    #[test]
    fn tiny_nonzero_sizes_produce_finite_lighting() {
        for size in [0., 1e-30, 0.01, 0.02, 0.021] {
            let surface = Surface::new(
                &[0., 1., 0.],
                3,
                1,
                &Settings {
                    size,
                    ..Default::default()
                },
            )
            .unwrap();
            for x in 0..3 {
                let sample = surface.sample(x, 0);
                assert!(sample.highlight.is_finite());
                assert!(sample.shadow.is_finite());
            }
        }
    }

    #[test]
    fn symmetric_padded_shape_keeps_overhead_lighting_symmetric() {
        let surface = Surface::new(
            &[0., 0., 1., 0., 0.],
            5,
            1,
            &Settings {
                size: 1.,
                altitude: 90.,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(surface.sample(0, 0), surface.sample(4, 0));
        assert_eq!(surface.sample(1, 0), surface.sample(3, 0));
        assert_eq!(surface.sample(2, 0), Shading::default());
    }
}
