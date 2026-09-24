//! Preview-only diagnostics. These options never enter the committed grading settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clipping {
    Shadows,
    Highlights,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub clipping: Option<Clipping>,
    pub shadow_overlay: bool,
    pub highlight_overlay: bool,
    pub point_color: Option<usize>,
    pub sharpen_mask: bool,
}
