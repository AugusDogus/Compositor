//! Full-resolution gradient cost. Run with `cargo run --release --example gradient_benchmark`.
use compositor::{
    Result,
    document::Document,
    gradient::{Gradient, Shape},
};
use std::time::Instant;

fn main() -> Result<()> {
    for (width, height) in [(1920, 1080), (3840, 2160)] {
        let original = Document::new(width, height)?;
        for shape in [Shape::Linear, Shape::Radial] {
            let gradient = Gradient {
                shape,
                ..Default::default()
            };
            let mut timings = Vec::new();
            for step in 0..8 {
                let mut document = original.clone();
                let started = Instant::now();
                gradient.apply(
                    &mut document,
                    [100., 100.],
                    [1000. + f64::from(step) * 50., 800.],
                    [25, 80, 230, 255],
                    [255; 4],
                    false,
                )?;
                timings.push(started.elapsed().as_secs_f64() * 1000.);
                std::hint::black_box(document);
            }
            println!("{width}x{height} {shape:?}: {timings:.1?} ms");
        }
    }
    Ok(())
}
