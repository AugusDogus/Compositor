use super::Characters;
use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap,
};

pub(super) struct Glyphs {
    pub maps: Vec<u8>,
    pub coverage: Vec<f32>,
    pub width: i32,
    pub height: i32,
}
impl Glyphs {
    pub fn empty() -> Self {
        Self {
            maps: Vec::new(),
            coverage: Vec::new(),
            width: 1,
            height: 1,
        }
    }
    pub fn render(characters: Characters, line_height: u8) -> Self {
        let mut fonts = FontSystem::new();
        let mut cache = SwashCache::new();
        let height = i32::from(line_height);
        let mut buffer = Buffer::new(
            &mut fonts,
            Metrics::new(f32::from(line_height) / 1.2, f32::from(line_height)),
        );
        buffer.set_wrap(Wrap::None);
        buffer.set_size(Some(128.), Some(f32::from(line_height)));
        let attrs = Attrs::new().family(Family::Monospace).weight(Weight::BOLD);
        buffer.set_text("M", &attrs, Shaping::Advanced, Some(Align::Left));
        buffer.shape_until_scroll(&mut fonts, false);
        let width = buffer
            .layout_runs()
            .next()
            .map_or(1, |run| run.line_w.round().max(1.) as i32);
        buffer.set_size(Some(width as f32), Some(height as f32));
        let text = characters.text();
        let text = if text.is_empty() {
            Characters::default().text()
        } else {
            text
        };
        let mut seen = std::collections::HashSet::new();
        let mut drawn = Vec::new();
        for character in text.chars().filter(|c| seen.insert(*c)) {
            buffer.set_text(
                &character.to_string(),
                &attrs,
                Shaping::Advanced,
                Some(Align::Center),
            );
            buffer.shape_until_scroll(&mut fonts, false);
            let mut map = vec![0_u8; (width * height) as usize];
            buffer.draw(
                &mut fonts,
                &mut cache,
                Color::rgb(255, 255, 255),
                |x, y, w, h, color| {
                    for py in y.max(0)..(y + h as i32).min(height) {
                        for px in x.max(0)..(x + w as i32).min(width) {
                            let target = &mut map[(py * width + px) as usize];
                            *target = (*target).max(color.a());
                        }
                    }
                },
            );
            let coverage = map.iter().map(|v| u32::from(*v)).sum::<u32>() as f32
                / (255 * width * height) as f32;
            drawn.push((map, coverage));
        }
        drawn.sort_by(|a, b| a.1.total_cmp(&b.1));
        Self {
            maps: drawn
                .iter()
                .flat_map(|(map, _)| map.iter().copied())
                .collect(),
            coverage: drawn.iter().map(|(_, coverage)| *coverage).collect(),
            width,
            height,
        }
    }
}
