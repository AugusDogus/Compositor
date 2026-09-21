use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use naga::{
    ShaderStage,
    front::wgsl,
    valid::{Capabilities, ValidationFlags, Validator},
};
use thiserror::Error;

/// Maximum application WGSL retained by one custom shader.
pub const MAX_CUSTOM_SHADER_SOURCE_BYTES: usize = 64 * 1024;
/// Number of `vec4<f32>` parameter slots supplied to every custom shader instance.
pub const CUSTOM_SHADER_PARAMETER_VECTORS: usize = 4;

const MAX_SHADER_ERROR_BYTES: usize = 8 * 1024;

const FRAGMENT_PREFIX: &str = r#"
struct QuickGuiShaderInput {
    uv: vec2<f32>,
    position: vec2<f32>,
    size: vec2<f32>,
    params: array<vec4<f32>, 4>,
}

struct QuickGuiFragmentInput {
    @builtin(position) physical_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) logical_position: vec2<f32>,
    @location(2) @interpolate(flat) logical_clip: vec4<f32>,
    @location(3) @interpolate(flat) size: vec2<f32>,
    @location(4) @interpolate(flat) params_0: vec4<f32>,
    @location(5) @interpolate(flat) params_1: vec4<f32>,
    @location(6) @interpolate(flat) params_2: vec4<f32>,
    @location(7) @interpolate(flat) params_3: vec4<f32>,
    @location(8) @interpolate(flat) opacity: f32,
}
"#;

const FRAGMENT_SUFFIX: &str = r#"
@fragment
fn fs_main(input: QuickGuiFragmentInput) -> @location(0) vec4<f32> {
    let position = input.logical_position;
    if position.x < input.logical_clip.x
        || position.y < input.logical_clip.y
        || position.x >= input.logical_clip.z
        || position.y >= input.logical_clip.w
    {
        discard;
    }
    let straight = quickgui_fragment(QuickGuiShaderInput(
        input.uv,
        input.logical_position,
        input.size,
        array<vec4<f32>, 4>(
            input.params_0,
            input.params_1,
            input.params_2,
            input.params_3,
        ),
    ));
    let alpha = clamp(straight.a * input.opacity, 0.0, 1.0);
    return vec4<f32>(straight.rgb * alpha, alpha);
}
"#;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum CustomShaderError {
    #[error("custom shader WGSL is empty")]
    Empty,
    #[error(
        "custom shader WGSL is {bytes} bytes; the maximum is {MAX_CUSTOM_SHADER_SOURCE_BYTES} bytes"
    )]
    SourceTooLarge { bytes: usize },
    #[error("custom shader WGSL is invalid: {0}")]
    InvalidWgsl(String),
    #[error("custom shaders cannot declare bind-group resources")]
    BoundResource,
    #[error("custom shaders cannot declare pipeline override constants")]
    PipelineOverride,
    #[error("custom shaders must only use QuickGUI's generated fragment entry point")]
    AdditionalEntryPoint,
}

struct CustomShaderInner {
    id: [u64; 2],
    source: Arc<str>,
}

/// Validated, immutable WGSL used by a retained custom GPU primitive.
///
/// Supply one function with this signature:
///
/// ```wgsl
/// fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
///     return vec4<f32>(input.uv, 0.0, 1.0);
/// }
/// ```
///
/// The returned color uses linear, straight alpha; QuickGUI clips and premultiplies it. Shader
/// sources are trusted application code. They are structurally validated and storage is bounded,
/// but—as with direct WGPU—an intentionally non-terminating shader can still reset a GPU.
#[derive(Clone)]
pub struct CustomShader(Arc<CustomShaderInner>);

impl CustomShader {
    pub fn new(source: impl AsRef<str>) -> Result<Self, CustomShaderError> {
        let source = source.as_ref();
        if source.trim().is_empty() {
            return Err(CustomShaderError::Empty);
        }
        if source.len() > MAX_CUSTOM_SHADER_SOURCE_BYTES {
            return Err(CustomShaderError::SourceTooLarge {
                bytes: source.len(),
            });
        }

        let mut complete =
            String::with_capacity(FRAGMENT_PREFIX.len() + source.len() + FRAGMENT_SUFFIX.len() + 2);
        complete.push_str(FRAGMENT_PREFIX);
        complete.push('\n');
        complete.push_str(source);
        complete.push('\n');
        complete.push_str(FRAGMENT_SUFFIX);

        let module = wgsl::parse_str(&complete).map_err(|error| {
            CustomShaderError::InvalidWgsl(bounded_error(error.emit_to_string(&complete)))
        })?;
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .map_err(|error| CustomShaderError::InvalidWgsl(bounded_error(error.to_string())))?;
        if module
            .global_variables
            .iter()
            .any(|(_, variable)| variable.binding.is_some())
        {
            return Err(CustomShaderError::BoundResource);
        }
        if !module.overrides.is_empty() {
            return Err(CustomShaderError::PipelineOverride);
        }
        if module.entry_points.len() != 1
            || module.entry_points[0].stage != ShaderStage::Fragment
            || module.entry_points[0].name != "fs_main"
        {
            return Err(CustomShaderError::AdditionalEntryPoint);
        }

        let source: Arc<str> = Arc::from(complete);
        let id = [
            stable_hash(&source, 0xcbf2_9ce4_8422_2325),
            stable_hash(&source, 0x6eed_0e9d_a4d9_4a4f),
        ];
        Ok(Self(Arc::new(CustomShaderInner { id, source })))
    }

    pub(crate) fn id(&self) -> [u64; 2] {
        self.0.id
    }

    pub(crate) fn source(&self) -> &str {
        &self.0.source
    }

    pub fn source_bytes(&self) -> usize {
        self.0
            .source
            .len()
            .saturating_sub(FRAGMENT_PREFIX.len() + FRAGMENT_SUFFIX.len() + 2)
    }
}

impl fmt::Debug for CustomShader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CustomShader")
            .field("id", &self.id())
            .field("source_bytes", &self.source_bytes())
            .finish()
    }
}

impl PartialEq for CustomShader {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.id() == other.id() && self.0.source == other.0.source)
    }
}

impl Eq for CustomShader {}

impl Hash for CustomShader {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

/// Four sanitized `vec4<f32>` slots passed without per-instance bind groups.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShaderParameters([[f32; 4]; CUSTOM_SHADER_PARAMETER_VECTORS]);

impl ShaderParameters {
    pub const fn new() -> Self {
        Self([[0.0; 4]; CUSTOM_SHADER_PARAMETER_VECTORS])
    }

    pub fn vector(mut self, slot: usize, value: [f32; 4]) -> Self {
        assert!(
            slot < CUSTOM_SHADER_PARAMETER_VECTORS,
            "custom shader parameter slot {slot} is out of range"
        );
        self.0[slot] = value.map(finite_or_zero);
        self
    }

    pub fn float(mut self, index: usize, value: f32) -> Self {
        assert!(
            index < CUSTOM_SHADER_PARAMETER_VECTORS * 4,
            "custom shader float parameter {index} is out of range"
        );
        self.0[index / 4][index % 4] = finite_or_zero(value);
        self
    }

    pub const fn vectors(self) -> [[f32; 4]; CUSTOM_SHADER_PARAMETER_VECTORS] {
        self.0
    }
}

impl From<[[f32; 4]; CUSTOM_SHADER_PARAMETER_VECTORS]> for ShaderParameters {
    fn from(value: [[f32; 4]; CUSTOM_SHADER_PARAMETER_VECTORS]) -> Self {
        value
            .into_iter()
            .enumerate()
            .fold(Self::new(), |parameters, (slot, value)| {
                parameters.vector(slot, value)
            })
    }
}

impl From<[f32; CUSTOM_SHADER_PARAMETER_VECTORS * 4]> for ShaderParameters {
    fn from(value: [f32; CUSTOM_SHADER_PARAMETER_VECTORS * 4]) -> Self {
        value
            .into_iter()
            .enumerate()
            .fold(Self::new(), |parameters, (index, value)| {
                parameters.float(index, value)
            })
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn stable_hash(source: &str, mut hash: u64) -> u64 {
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn bounded_error(mut error: String) -> String {
    if error.len() <= MAX_SHADER_ERROR_BYTES {
        return error;
    }
    let mut end = MAX_SHADER_ERROR_BYTES;
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error.truncate(end);
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
    return vec4<f32>(input.uv, input.params[0].x, 1.0);
}
"#;

    #[test]
    fn validates_the_fixed_fragment_contract_and_clones_identity() {
        let shader = CustomShader::new(VALID).unwrap();
        let clone = shader.clone();

        assert_eq!(shader, clone);
        assert_eq!(shader.source_bytes(), VALID.len());
        assert_eq!(shader.id(), clone.id());
    }

    #[test]
    fn rejects_invalid_interfaces_resources_and_unbounded_source() {
        assert_eq!(
            CustomShader::new("  ").unwrap_err(),
            CustomShaderError::Empty
        );
        assert!(matches!(
            CustomShader::new("fn wrong() {}").unwrap_err(),
            CustomShaderError::InvalidWgsl(_)
        ));
        assert_eq!(
            CustomShader::new(
                r#"
@group(1) @binding(0) var texture: texture_2d<f32>;
fn quickgui_fragment(input: QuickGuiShaderInput) -> vec4<f32> {
    return vec4<f32>(input.uv, 0.0, 1.0);
}
"#,
            )
            .unwrap_err(),
            CustomShaderError::BoundResource
        );
        assert!(matches!(
            CustomShader::new("x".repeat(MAX_CUSTOM_SHADER_SOURCE_BYTES + 1)).unwrap_err(),
            CustomShaderError::SourceTooLarge { .. }
        ));
    }

    #[test]
    fn parameters_sanitize_non_finite_values_without_heap_storage() {
        let parameters = ShaderParameters::new()
            .vector(0, [1.0, f32::NAN, f32::INFINITY, -2.0])
            .float(7, 3.0);

        assert_eq!(parameters.vectors()[0], [1.0, 0.0, 0.0, -2.0]);
        assert_eq!(parameters.vectors()[1][3], 3.0);
        assert_eq!(
            std::mem::size_of::<ShaderParameters>(),
            CUSTOM_SHADER_PARAMETER_VECTORS * 16
        );
    }
}
