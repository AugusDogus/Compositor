//! Project color runs use UTF-16 offsets. Editor selections use UTF-8 byte offsets.
use super::Text;
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorRun {
    pub location: usize,
    pub length: usize,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}

impl ColorRun {
    fn rgb(&self) -> [f64; 3] {
        [self.red, self.green, self.blue]
    }
}

impl Text {
    pub fn base_color(&self) -> [f64; 3] {
        [self.red, self.green, self.blue]
    }

    pub(super) fn validate_colors(&self) -> Result<()> {
        let Some(runs) = &self.color_runs else {
            return Ok(());
        };
        let count = self.content.encode_utf16().count();
        // A boundary map keeps validation linear even for 100,000 alternating runs.
        let mut boundaries = vec![true; count + 1];
        let mut unit = 0;
        for ch in self.content.chars() {
            if ch.len_utf16() == 2 {
                boundaries[unit + 1] = false;
            }
            unit += ch.len_utf16();
        }
        let mut end = 0;
        let valid = !runs.is_empty()
            && runs.len() <= count
            && runs.iter().all(|run| {
                let Some(next) = run.location.checked_add(run.length) else {
                    return false;
                };
                let valid = run.length > 0
                    && run.location >= end
                    && next <= count
                    && boundaries.get(run.location) == Some(&true)
                    && boundaries.get(next) == Some(&true)
                    && run.rgb().iter().all(|c| (0. ..=1.).contains(c));
                end = next;
                valid
            });
        if valid {
            Ok(())
        } else {
            Err(invalid(
                "Text color ranges must be ordered, non-overlapping character ranges within the text, with RGB values from 0 to 1.",
            ))
        }
    }

    /// Colors at the caret follow the preceding character, or the first at offset zero.
    pub fn caret_color(&self, byte: usize) -> [f64; 3] {
        let byte = byte.min(self.content.len());
        let units = self
            .content
            .char_indices()
            .take_while(|(i, _)| *i < byte)
            .map(|(_, c)| c.len_utf16())
            .sum::<usize>();
        self.color_at_unit(units.saturating_sub(1))
    }

    fn color_at_unit(&self, unit: usize) -> [f64; 3] {
        self.color_runs
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|r| unit >= r.location && unit - r.location < r.length)
            .map_or_else(|| self.base_color(), ColorRun::rgb)
    }

    /// Complete UTF-8 spans, including the inherited base color, ready for a text shaper.
    pub fn colored_spans(&self) -> Vec<(Range<usize>, [f64; 3])> {
        let mut spans: Vec<(Range<usize>, [f64; 3])> = Vec::new();
        let mut runs = self
            .color_runs
            .as_deref()
            .unwrap_or_default()
            .iter()
            .peekable();
        let mut unit = 0;
        for (byte, ch) in self.content.char_indices() {
            while runs
                .peek()
                .is_some_and(|r| unit >= r.location.saturating_add(r.length))
            {
                runs.next();
            }
            let rgb = runs
                .peek()
                .filter(|r| unit >= r.location)
                .map_or_else(|| self.base_color(), |r| r.rgb());
            let end = byte + ch.len_utf8();
            if let Some((range, previous)) = spans.last_mut()
                && *previous == rgb
            {
                range.end = end;
            } else {
                spans.push((byte..end, rgb));
            }
            unit += ch.len_utf16();
        }
        spans
    }

    /// Empty selection recolors the layer, matching upstream. Otherwise only selected letters change.
    pub fn set_color(&mut self, bytes: Range<usize>, rgb: [f64; 3]) -> Result<()> {
        self.check_range(&bytes)?;
        if !rgb.iter().all(|c| (0. ..=1.).contains(c)) {
            return Err(invalid("Text color channels must be between 0 and 1."));
        }
        if bytes.is_empty() || bytes == (0..self.content.len()) {
            [self.red, self.green, self.blue] = rgb;
            self.color_runs = None;
        } else {
            let start = self.content[..bytes.start].encode_utf16().count();
            let end = start + self.content[bytes].encode_utf16().count();
            let mut colors = self.unit_colors();
            colors[start..end].fill(rgb);
            self.set_unit_colors(&colors);
        }
        Ok(())
    }

    pub fn replace_characters(&mut self, bytes: Range<usize>, replacement: &str) -> Result<()> {
        self.check_range(&bytes)?;
        let start = self.content[..bytes.start].encode_utf16().count();
        let end = start + self.content[bytes.clone()].encode_utf16().count();
        let length = replacement.encode_utf16().count();
        if self.content.encode_utf16().count() - (end - start) + length > 100_000 {
            return Err(invalid(
                "Text is limited to 100,000 UTF-16 units. Shorten the inserted text.",
            ));
        }
        if self.color_runs.is_some() {
            let mut colors = self.unit_colors();
            let inherited = colors
                .get(start.saturating_sub(1))
                .copied()
                .unwrap_or_else(|| self.base_color());
            colors.splice(start..end, std::iter::repeat_n(inherited, length));
            self.set_unit_colors(&colors);
        }
        self.content.replace_range(bytes, replacement);
        Ok(())
    }

    fn check_range(&self, bytes: &Range<usize>) -> Result<()> {
        if bytes.start > bytes.end || self.content.get(bytes.clone()).is_none() {
            return Err(invalid(
                "The text selection is outside the current text. Select the letters again.",
            ));
        }
        self.validate()
    }

    fn unit_colors(&self) -> Vec<[f64; 3]> {
        let mut colors = vec![self.base_color(); self.content.encode_utf16().count()];
        for run in self.color_runs.as_deref().unwrap_or_default() {
            colors[run.location..run.location + run.length].fill(run.rgb());
        }
        colors
    }

    fn set_unit_colors(&mut self, colors: &[[f64; 3]]) {
        let base = self.base_color();
        let mut runs: Vec<ColorRun> = Vec::new();
        for (location, rgb) in colors
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, c)| *c != base)
        {
            if let Some(last) = runs.last_mut()
                && last.location + last.length == location
                && last.rgb() == rgb
            {
                last.length += 1;
            } else {
                runs.push(ColorRun {
                    location,
                    length: 1,
                    red: rgb[0],
                    green: rgb[1],
                    blue: rgb[2],
                });
            }
        }
        self.color_runs = (!runs.is_empty()).then_some(runs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_runs_follow_unicode_edits_and_typed_color() {
        let mut text = Text {
            content: "A🙂中Z".into(),
            ..Text::default()
        };
        text.set_color(1..5, [1., 0., 0.]).unwrap();
        assert_eq!(text.color_runs.as_ref().unwrap()[0].length, 2);
        text.replace_characters(5..8, "é").unwrap();
        assert_eq!(text.content, "A🙂éZ");
        assert_eq!(text.color_runs.as_ref().unwrap()[0].length, 3);
        assert_eq!(text.colored_spans()[1], (1..7, [1., 0., 0.]));
        text.validate().unwrap();
        assert!(text.set_color(2..4, [0.; 3]).is_err());
        text.set_color(0..0, [0., 0., 1.]).unwrap();
        assert!(text.color_runs.is_none());
    }

    #[test]
    fn rejects_overflow_overlap_and_surrogate_splits() {
        let mut text = Text {
            content: "🙂x".into(),
            ..Text::default()
        };
        let run = ColorRun {
            location: 0,
            length: 2,
            red: 1.,
            green: 0.,
            blue: 0.,
        };
        text.color_runs = Some(vec![run.clone()]);
        text.validate().unwrap();
        for (location, length) in [(1, 1), (2, 2), (usize::MAX, 1), (0, 0)] {
            text.color_runs = Some(vec![ColorRun {
                location,
                length,
                ..run.clone()
            }]);
            assert!(text.validate().is_err());
        }
        text.color_runs = Some(vec![run.clone(), run]);
        assert!(text.validate().is_err());
        text.color_runs = Some(vec![]);
        assert!(text.validate().is_err());
    }
}
