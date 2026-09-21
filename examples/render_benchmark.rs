//! Viewport compositing timings, including cold and cached high-quality reductions.
use compositor::{
    adjustment::{Adjustment, Kind},
    document::{Document, Layer, LayerContent},
    geometry::Sampling,
    render,
};
use image::{Rgba, RgbaImage};
use std::{sync::Arc, time::Instant};
fn main() {
    let cpu = std::env::args().any(|arg| arg == "--cpu");
    if !cpu {
        let at = Instant::now();
        render::initialize_gpu().unwrap();
        println!("GPU startup={:.1}ms", at.elapsed().as_secs_f64() * 1000.);
    }

    for (count, adjustment_count) in [(1, 0), (8, 0), (1, 8)] {
        let mut doc = Document::new(4000, 4000).unwrap();
        doc.layers.clear();
        let source = Arc::new(RgbaImage::from_fn(4000, 4000, |x, y| {
            Rgba([x as u8, y as u8, (x + y) as u8, 200])
        }));
        for i in 0..count {
            let mut layer = Layer::blank(format!("Layer {i}"), 4000, 4000);
            layer.content = LayerContent::Raster(Some(source.clone()));
            layer.transform.sampling = Sampling::High;
            layer.opacity = 0.6;
            doc.layers.push(layer);
        }
        for i in 0..adjustment_count {
            let mut layer = Layer::blank(format!("Adjustment {i}"), 4000, 4000);
            let mut settings = Adjustment::new(
                [
                    Kind::HueSaturation,
                    Kind::Levels,
                    Kind::Curves,
                    Kind::Exposure,
                    Kind::GradientMap,
                    Kind::Grain,
                ][i % 6],
            );
            settings.hue = 25.;
            settings.levels.ranges[0].gamma = 0.8;
            layer.content = LayerContent::Adjustment(Box::new(settings));
            layer.opacity = 0.7;
            doc.layers.push(layer);
        }
        let mut cache = render::DownsampleCache::default();
        for pass in 0..4 {
            let start = Instant::now();
            if pass == 3
                && let LayerContent::Raster(Some(pixels)) = &mut doc.layers[0].content
            {
                Arc::make_mut(pixels).put_pixel(2000, 2000, Rgba([255; 4]));
            }
            let result = if cpu {
                Ok(render::region_cached(
                    &doc,
                    1200,
                    900,
                    [0., 0.],
                    [4000. / 1200.; 2],
                    &mut cache,
                ))
            } else {
                render::region_accelerated(
                    &doc,
                    1200,
                    900,
                    [0., 0.],
                    [4000. / 1200.; 2],
                    &mut cache,
                )
            };
            std::hint::black_box(result.unwrap());
            println!(
                "layers={count} adjustments={adjustment_count} pass={pass} viewport={:.1}ms",
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
}
