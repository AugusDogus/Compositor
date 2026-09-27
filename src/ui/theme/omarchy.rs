use super::palette::Palette;
use compositor::{Result, invalid};
use std::path::{Path, PathBuf};
const LIMIT: u64 = 64 * 1024;

pub(super) fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
        .map(|p| p.join("omarchy/current/theme/colors.toml"))
}

pub(super) fn read(path: &Path) -> Result<Palette> {
    let text = super::read_text(path, LIMIT)?;
    parse(&text)
}

// Omarchy colors.toml consists of flat quoted color assignments. Read only the
// three required chrome colors; unknown keys cannot alter files or run commands.
fn parse(text: &str) -> Result<Palette> {
    let mut background = None;
    let mut foreground = None;
    let mut accent = None;
    let mut in_section = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = true;
        }
        if in_section || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let target = match key.trim() {
            "background" => &mut background,
            "foreground" => &mut foreground,
            "accent" => &mut accent,
            _ => continue,
        };
        if target.is_some() {
            return Err(invalid("The Omarchy palette repeats a required color."));
        }
        let value = value.trim();
        let quote = value
            .chars()
            .next()
            .filter(|c| matches!(c, '\'' | '"'))
            .ok_or_else(|| invalid("Omarchy colors must be quoted #RRGGBB values."))?;
        let (hex, rest) = value[1..]
            .split_once(quote)
            .ok_or_else(|| invalid("An Omarchy color has no closing quote."))?;
        if !(rest.trim().is_empty() || rest.trim().starts_with('#'))
            || hex.len() != 7
            || !hex.starts_with('#')
            || !hex[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("Omarchy colors must be quoted #RRGGBB values."));
        }
        let mut rgb = [0; 3];
        for (i, value) in rgb.iter_mut().enumerate() {
            *value = u8::from_str_radix(&hex[1 + 2 * i..3 + 2 * i], 16)
                .map_err(|_| invalid("Invalid Omarchy color."))?;
        }
        *target = Some(rgb);
    }
    match (background, foreground, accent) {
        (Some(background), Some(foreground), Some(accent)) => Ok(Palette {
            background,
            foreground,
            accent,
        }),
        _ => Err(invalid(
            "The Omarchy palette needs background, foreground and accent colors.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_flat_palette_and_rejects_malformed_or_duplicate_required_colors() {
        let input = "background = '#eeeeee'\nforeground = \"#102030\" # comment\naccent = '#abcdef'\ncolor1 = '#000000'\n";
        let palette = parse(input).unwrap();
        assert_eq!(palette.background, [238; 3]);
        assert_eq!(palette.foreground, [16, 32, 48]);
        assert_eq!(palette.accent, [171, 205, 239]);
        assert!(parse(&format!("{input}accent = '#000000'\n")).is_err());
        assert!(parse(&input.replace("#eeeeee", "#éeeee")).is_err());
        assert!(parse("background = '#000000'\n").is_err());
        assert!(parse(&format!("[unrelated]\n{input}")).is_err());
    }
    #[test]
    fn follows_replaced_theme_symlink_and_bounds_file_reads() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        let path = temp.path().join("current");
        std::fs::write(
            &a,
            "background='#000000'\nforeground='#ffffff'\naccent='#123456'",
        )
        .unwrap();
        std::fs::write(
            &b,
            "background='#ffffff'\nforeground='#000000'\naccent='#654321'",
        )
        .unwrap();
        std::os::unix::fs::symlink(&a, &path).unwrap();
        assert_eq!(read(&path).unwrap().background, [0; 3]);
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&b, &path).unwrap();
        assert_eq!(read(&path).unwrap().background, [255; 3]);
        std::fs::write(&b, vec![b' '; LIMIT as usize + 1]).unwrap();
        assert!(read(&path).is_err());
    }
}
