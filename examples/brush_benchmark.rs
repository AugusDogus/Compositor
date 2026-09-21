//! Vulkan-accelerated stroke timings matching the 4K paths in macOS BrushPerformanceTests.
//! Run with `cargo run --release --example brush_benchmark`.
use compositor::{
    Result,
    brush::{Brush, PaintMode, Stroke},
    document::{Document, LayerContent},
};
use image::{Rgba, RgbaImage};
use std::{sync::Arc, time::Instant};

fn main() -> Result<()> {
    println!("Brush device: {}", compositor::brush::initialize_gpu()?);
    for diameter in [40., 800.] {
        for opaque in [false, true] {
            let mut doc = Document::new(4000, 4000)?;
            if opaque {
                doc.layers[0].content = LayerContent::Raster(Some(Arc::new(
                    RgbaImage::from_pixel(4000, 4000, Rgba([0, 0, 0, 255])),
                )));
            }
            let brush = Brush {
                diameter,
                hardness: 0.,
                color: [255; 4],
                ..Brush::default()
            };
            for pass in 0..2 {
                let started = Instant::now();
                let mut stroke = Stroke::start(
                    &mut doc,
                    [700., 3200.],
                    brush,
                    PaintMode::Paint,
                    false,
                    false,
                )?;
                let mut frames = Vec::new();
                for i in 1..=120 {
                    let at = Instant::now();
                    let point = if i <= 60 {
                        [700., 3200. - f64::from(i) * 40.]
                    } else {
                        [700. + f64::from(i - 60) * 40., 800.]
                    };
                    stroke.to(&mut doc, point)?;
                    frames.push(at.elapsed().as_secs_f64() * 1000.);
                }
                let drawing = started.elapsed();
                let at = Instant::now();
                stroke.finish(&mut doc)?;
                frames.sort_by(f64::total_cmp);
                println!(
                    "diameter={diameter} opaque={opaque} pass={pass} draw={:.0}ms finish={:.1}ms frame median={:.1}ms p95={:.1}ms max={:.1}ms",
                    drawing.as_secs_f64() * 1000.,
                    at.elapsed().as_secs_f64() * 1000.,
                    frames[60],
                    frames[114],
                    frames[119]
                );
                doc.validate()?;
            }
        }
    }
    Ok(())
}
