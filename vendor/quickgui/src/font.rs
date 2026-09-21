use std::{
    fmt,
    hash::{Hash, Hasher},
    mem::size_of,
    sync::{Arc, LazyLock},
};

use glyphon::{
    Style as GlyphStyle, Weight,
    cosmic_text::{
        Family as GlyphFamily, FamilyOwned as GlyphFamilyOwned, FeatureTag as GlyphFeatureTag,
        FontFeatures as GlyphFontFeatures,
    },
};

/// Maximum UTF-8 size accepted for one named font family.
pub const MAX_FONT_FAMILY_BYTES: usize = 1_024;
/// Maximum number of ordered custom fallback families retained by one font style.
pub const MAX_FONT_FALLBACKS: usize = 8;
/// Maximum number of explicit OpenType feature values retained by one font style.
pub const MAX_FONT_FEATURES: usize = 32;

/// A generic or explicitly named font family.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum FontFamily {
    SansSerif,
    Serif,
    Monospace,
    Named(Arc<str>),
}

impl FontFamily {
    /// Create a named family after enforcing QuickGUI's retained-string bound.
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        let name = name.into();
        assert_valid_family_name(&name);
        Self::Named(name)
    }
}

impl From<&str> for FontFamily {
    fn from(value: &str) -> Self {
        Self::named(value)
    }
}

impl From<String> for FontFamily {
    fn from(value: String) -> Self {
        Self::named(value)
    }
}

impl From<Arc<str>> for FontFamily {
    fn from(value: Arc<str>) -> Self {
        Self::named(value)
    }
}

/// Ordered named families tried after the primary family and before platform fallbacks.
///
/// Clones share both the public family table and the precomputed Cosmic Text representation.
#[derive(Clone)]
pub struct FontFallbacks(Arc<FontFallbackData>);

struct FontFallbackData {
    families: Vec<Arc<str>>,
    cosmic: Arc<[GlyphFamilyOwned]>,
}

static EMPTY_FONT_FALLBACKS: LazyLock<Arc<FontFallbackData>> = LazyLock::new(|| {
    Arc::new(FontFallbackData {
        families: Vec::new(),
        cosmic: Arc::from([]),
    })
});

impl FontFallbacks {
    pub fn new() -> Self {
        Self(Arc::clone(&EMPTY_FONT_FALLBACKS))
    }

    /// Build a canonical fallback stack. Duplicate names are removed in declaration order.
    ///
    /// # Panics
    ///
    /// Panics when a name is empty or above [`MAX_FONT_FAMILY_BYTES`], or when more than
    /// [`MAX_FONT_FALLBACKS`] distinct families are supplied.
    pub fn from_fonts<I, S>(fonts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        let fonts = fonts.into_iter();
        let mut families = Vec::with_capacity(fonts.size_hint().0.min(MAX_FONT_FALLBACKS));
        for family in fonts {
            let family = family.into();
            assert_valid_family_name(&family);
            if families.iter().any(|current| current == &family) {
                continue;
            }
            assert!(
                families.len() < MAX_FONT_FALLBACKS,
                "a font supports at most {MAX_FONT_FALLBACKS} custom fallback families"
            );
            families.push(family);
        }
        if families.is_empty() {
            return Self::new();
        }
        let cosmic = families
            .iter()
            .map(|family| GlyphFamilyOwned::new(GlyphFamily::Name(family)))
            .collect::<Vec<_>>()
            .into();
        Self(Arc::new(FontFallbackData { families, cosmic }))
    }

    pub fn fallback_list(&self) -> &[Arc<str>] {
        &self.0.families
    }

    pub fn is_empty(&self) -> bool {
        self.0.families.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.families.len()
    }

    pub(crate) fn cosmic(&self) -> Arc<[GlyphFamilyOwned]> {
        Arc::clone(&self.0.cosmic)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        let names = self
            .0
            .families
            .iter()
            .fold(0_usize, |bytes, family| bytes.saturating_add(family.len()));
        size_of::<FontFallbackData>()
            .saturating_add(2 * size_of::<usize>())
            .saturating_add(
                self.0
                    .families
                    .capacity()
                    .saturating_mul(size_of::<Arc<str>>()),
            )
            .saturating_add(
                self.0
                    .cosmic
                    .len()
                    .saturating_mul(size_of::<GlyphFamilyOwned>()),
            )
            .saturating_add(self.0.families.len().saturating_mul(2 * size_of::<usize>()))
            .saturating_add(names.saturating_mul(2))
    }
}

impl PartialEq for FontFallbacks {
    fn eq(&self, other: &Self) -> bool {
        self.0.families == other.0.families
    }
}

impl Eq for FontFallbacks {}

impl Hash for FontFallbacks {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.families.hash(state);
    }
}

impl Default for FontFallbacks {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FontFallbacks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("FontFallbacks")
            .field(&self.0.families)
            .finish()
    }
}

/// A validated four-byte OpenType feature tag.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FontFeatureTag([u8; 4]);

impl FontFeatureTag {
    pub const KERNING: Self = Self(*b"kern");
    pub const STANDARD_LIGATURES: Self = Self(*b"liga");
    pub const CONTEXTUAL_LIGATURES: Self = Self(*b"clig");
    pub const CONTEXTUAL_ALTERNATES: Self = Self(*b"calt");
    pub const DISCRETIONARY_LIGATURES: Self = Self(*b"dlig");
    pub const SMALL_CAPS: Self = Self(*b"smcp");
    pub const ALL_SMALL_CAPS: Self = Self(*b"c2sc");
    pub const TABULAR_NUMBERS: Self = Self(*b"tnum");
    pub const PROPORTIONAL_NUMBERS: Self = Self(*b"pnum");
    pub const OLDSTYLE_NUMBERS: Self = Self(*b"onum");
    pub const LINING_NUMBERS: Self = Self(*b"lnum");
    pub const SLASHED_ZERO: Self = Self(*b"zero");
    pub const FRACTIONS: Self = Self(*b"frac");
    pub const ORDINALS: Self = Self(*b"ordn");
    pub const STYLISTIC_SET_1: Self = Self(*b"ss01");
    pub const STYLISTIC_SET_2: Self = Self(*b"ss02");

    pub const fn as_bytes(self) -> [u8; 4] {
        self.0
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("validated OpenType tags are ASCII")
    }
}

impl fmt::Debug for FontFeatureTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("FontFeatureTag")
            .field(&self.as_str())
            .finish()
    }
}

impl fmt::Display for FontFeatureTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Error returned when an OpenType feature tag is not four ASCII alphanumeric bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontFeatureTagError;

impl fmt::Display for FontFeatureTagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an OpenType feature tag must contain four ASCII letters or digits")
    }
}

impl std::error::Error for FontFeatureTagError {}

impl TryFrom<&str> for FontFeatureTag {
    type Error = FontFeatureTagError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let bytes: [u8; 4] = value
            .as_bytes()
            .try_into()
            .map_err(|_| FontFeatureTagError)?;
        if bytes.iter().all(u8::is_ascii_alphanumeric) {
            Ok(Self(bytes))
        } else {
            Err(FontFeatureTagError)
        }
    }
}

impl TryFrom<String> for FontFeatureTag {
    type Error = FontFeatureTagError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

/// One explicit OpenType feature value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FontFeature {
    pub tag: FontFeatureTag,
    pub value: u32,
}

/// Canonical, shared, and hard-bounded OpenType feature configuration.
#[derive(Clone, Default, Eq, Hash, PartialEq)]
pub struct FontFeatures(Option<Arc<Vec<FontFeature>>>);

impl FontFeatures {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a feature value. Repeated tags replace their previous value and the retained table is
    /// sorted by tag so equivalent declarations share the same text-layout cache key.
    ///
    /// # Panics
    ///
    /// Panics when more than [`MAX_FONT_FEATURES`] distinct tags are supplied.
    pub fn set(self, tag: FontFeatureTag, value: u32) -> Self {
        let mut features = self.0.unwrap_or_else(|| Arc::new(Vec::new()));
        if let Some(index) = features.iter().position(|feature| feature.tag == tag) {
            if features[index].value == value {
                return Self(Some(features));
            }
            Arc::make_mut(&mut features)[index].value = value;
        } else {
            assert!(
                features.len() < MAX_FONT_FEATURES,
                "a font supports at most {MAX_FONT_FEATURES} explicit OpenType features"
            );
            let features = Arc::make_mut(&mut features);
            features.push(FontFeature { tag, value });
            features.sort_unstable_by_key(|feature| feature.tag);
        }
        Self(Some(features))
    }

    pub fn enable(self, tag: FontFeatureTag) -> Self {
        self.set(tag, 1)
    }

    pub fn disable(self, tag: FontFeatureTag) -> Self {
        self.set(tag, 0)
    }

    /// Match GPUI's convenience behavior by disabling contextual alternates (`calt`).
    pub fn disable_ligatures() -> Self {
        Self::new().disable(FontFeatureTag::CONTEXTUAL_ALTERNATES)
    }

    pub fn tag_value_list(&self) -> &[FontFeature] {
        self.0.as_deref().map(Vec::as_slice).unwrap_or_default()
    }

    pub fn value(&self, tag: FontFeatureTag) -> Option<u32> {
        self.tag_value_list()
            .binary_search_by_key(&tag, |feature| feature.tag)
            .ok()
            .map(|index| self.tag_value_list()[index].value)
    }

    pub fn is_calt_enabled(&self) -> Option<bool> {
        self.value(FontFeatureTag::CONTEXTUAL_ALTERNATES)
            .map(|value| value == 1)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    pub fn len(&self) -> usize {
        self.tag_value_list().len()
    }

    pub(crate) fn cosmic(&self) -> GlyphFontFeatures {
        let mut features = GlyphFontFeatures::new();
        features.features.reserve(self.len());
        for feature in self.tag_value_list() {
            let tag = feature.tag.as_bytes();
            features.set(GlyphFeatureTag::new(&tag), feature.value);
        }
        features
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.0.as_ref().map_or(0, |features| {
            size_of::<Vec<FontFeature>>()
                .saturating_add(2 * size_of::<usize>())
                .saturating_add(features.capacity().saturating_mul(size_of::<FontFeature>()))
        })
    }
}

impl fmt::Debug for FontFeatures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("FontFeatures")
            .field(&self.tag_value_list())
            .finish()
    }
}

impl FromIterator<(FontFeatureTag, u32)> for FontFeatures {
    fn from_iter<T: IntoIterator<Item = (FontFeatureTag, u32)>>(iter: T) -> Self {
        iter.into_iter()
            .fold(Self::new(), |features, (tag, value)| {
                features.set(tag, value)
            })
    }
}

/// Complete inherited font configuration used by [`crate::Element::font`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Font {
    pub family: FontFamily,
    pub features: FontFeatures,
    pub fallbacks: Option<FontFallbacks>,
    pub weight: Weight,
    pub style: GlyphStyle,
}

impl Font {
    pub fn new(family: impl Into<FontFamily>) -> Self {
        let family = family.into();
        assert_valid_font_family(&family);
        Self {
            family,
            features: FontFeatures::new(),
            fallbacks: None,
            weight: Weight::NORMAL,
            style: GlyphStyle::Normal,
        }
    }

    pub fn features(mut self, features: FontFeatures) -> Self {
        self.features = features;
        self
    }

    pub fn fallbacks(mut self, fallbacks: FontFallbacks) -> Self {
        self.fallbacks = (!fallbacks.is_empty()).then_some(fallbacks);
        self
    }

    pub fn weight(mut self, weight: Weight) -> Self {
        self.weight = weight;
        self
    }

    pub fn style(mut self, style: GlyphStyle) -> Self {
        self.style = style;
        self
    }

    pub fn bold(mut self) -> Self {
        self.weight = Weight::BOLD;
        self
    }

    pub fn italic(mut self) -> Self {
        self.style = GlyphStyle::Italic;
        self
    }
}

impl Default for Font {
    fn default() -> Self {
        Self::new(FontFamily::SansSerif)
    }
}

/// Construct a complete font configuration from a family name or generic [`FontFamily`].
pub fn font(family: impl Into<FontFamily>) -> Font {
    Font::new(family)
}

pub(crate) fn glyph_family(family: &FontFamily) -> GlyphFamily<'_> {
    match family {
        FontFamily::SansSerif => GlyphFamily::SansSerif,
        FontFamily::Serif => GlyphFamily::Serif,
        FontFamily::Monospace => GlyphFamily::Monospace,
        FontFamily::Named(name) => GlyphFamily::Name(name),
    }
}

pub(crate) fn assert_valid_font_family(family: &FontFamily) {
    if let FontFamily::Named(name) = family {
        assert_valid_family_name(name);
    }
}

pub(crate) fn normalize_fallbacks(fallbacks: Option<FontFallbacks>) -> Option<FontFallbacks> {
    fallbacks.filter(|fallbacks| !fallbacks.is_empty())
}

fn assert_valid_family_name(name: &str) {
    assert!(!name.is_empty(), "font family names cannot be empty");
    assert!(
        name.len() <= MAX_FONT_FAMILY_BYTES,
        "font family names support at most {MAX_FONT_FAMILY_BYTES} UTF-8 bytes"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_are_validated_canonical_and_bounded() {
        assert_eq!(
            FontFeatureTag::try_from("calt").unwrap(),
            FontFeatureTag::CONTEXTUAL_ALTERNATES
        );
        assert!(FontFeatureTag::try_from("abc").is_err());
        assert!(FontFeatureTag::try_from("ab-c").is_err());

        let features = FontFeatures::new()
            .enable(FontFeatureTag::STANDARD_LIGATURES)
            .disable(FontFeatureTag::CONTEXTUAL_ALTERNATES)
            .disable(FontFeatureTag::STANDARD_LIGATURES);
        assert_eq!(features.len(), 2);
        assert_eq!(features.value(FontFeatureTag::STANDARD_LIGATURES), Some(0));
        assert_eq!(features.is_calt_enabled(), Some(false));
        assert_eq!(features, features.clone());
        let unchanged = features.clone().disable(FontFeatureTag::STANDARD_LIGATURES);
        assert!(Arc::ptr_eq(
            features.0.as_ref().unwrap(),
            unchanged.0.as_ref().unwrap()
        ));
        assert_eq!(
            FontFeatures::new()
                .enable(FontFeatureTag::STANDARD_LIGATURES)
                .disable(FontFeatureTag::CONTEXTUAL_ALTERNATES),
            FontFeatures::new()
                .disable(FontFeatureTag::CONTEXTUAL_ALTERNATES)
                .enable(FontFeatureTag::STANDARD_LIGATURES)
        );
    }

    #[test]
    fn fallback_stacks_deduplicate_and_precompute_cosmic_families() {
        let fallbacks = FontFallbacks::from_fonts([
            "Symbols Nerd Font",
            "Apple Color Emoji",
            "Symbols Nerd Font",
        ]);
        assert_eq!(fallbacks.len(), 2);
        assert_eq!(fallbacks.fallback_list()[0].as_ref(), "Symbols Nerd Font");
        assert_eq!(fallbacks.cosmic().len(), 2);
    }

    #[test]
    fn shared_font_tables_keep_thin_handles_in_every_text_style() {
        assert_eq!(size_of::<FontFeatures>(), size_of::<usize>());
        assert_eq!(size_of::<FontFallbacks>(), size_of::<usize>());
        assert!(FontFeatures::new().is_empty());
    }

    #[test]
    #[should_panic(expected = "at most 8 custom fallback families")]
    fn fallback_stacks_have_a_hard_count_bound() {
        let _ = FontFallbacks::from_fonts(
            (0..=MAX_FONT_FALLBACKS).map(|index| format!("Font {index}")),
        );
    }

    #[test]
    #[should_panic(expected = "at most 32 explicit OpenType features")]
    fn feature_tables_have_a_hard_count_bound() {
        let mut features = FontFeatures::new();
        for index in 0..=MAX_FONT_FEATURES {
            let tag = format!("x{index:03}");
            features = features.set(FontFeatureTag::try_from(tag).unwrap(), 1);
        }
    }
}
