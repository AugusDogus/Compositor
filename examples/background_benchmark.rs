//! Measure first and cached native removals: cargo run --release --example background_benchmark -- photo.png
use compositor::{background::SubjectMask, document::Document, image_io, invalid};
use std::{path::Path, time::Instant};

fn main() -> compositor::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| invalid("Pass a subject photo path."))?;
    let layer = image_io::import(Path::new(&path))?;
    let pixels = layer
        .raster()
        .ok_or_else(|| invalid("The photo contains no raster pixels."))?;
    let mut document = Document::new(pixels.width(), pixels.height())?;
    document.layers.clear();
    document.add(layer)?;
    for run in 0..4 {
        let start = Instant::now();
        std::hint::black_box(SubjectMask::detect(&document)?);
        println!(
            "removal={run} elapsed_ms={:.1}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
    Ok(())
}
