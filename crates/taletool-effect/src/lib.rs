//! Typed effect assets stored in `NSedData`, `NSeffData`, `NSemData`, and
//! `NSesData` archives.

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// Byte length of a color or texture animation header.
pub const VALUE_ANIMATION_HEADER_LEN: usize = 0x0a;
/// Byte length of a transform animation header.
pub const TRANSFORM_ANIMATION_HEADER_LEN: usize = 0x14;
/// Byte length of an effect-definition root header.
pub const EFFECT_DEFINITION_HEADER_LEN: usize = 0x18;
/// Byte length of one fixed effect component record.
pub const EFFECT_COMPONENT_RECORD_LEN: usize = 0xc0;
/// Byte length of the non-track portion of an effect component.
pub const EFFECT_COMPONENT_FIXED_DATA_LEN: usize = 0x60;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum EffectError {
    #[error(transparent)]
    Truncated(#[from] ByteReadError),
    #[error("{asset} payload has {count} trailing bytes")]
    TrailingBytes { asset: &'static str, count: usize },
    #[error(
        "animation timing is invalid: first={first_frame}, last={last_frame}, rate={frame_rate}, step={keyframe_step}"
    )]
    InvalidTiming {
        first_frame: i16,
        last_frame: i16,
        frame_rate: i16,
        keyframe_step: i16,
    },
    #[error(
        "{track} keyframes are not strictly ordered at index {index}: {previous} then {current}"
    )]
    UnorderedKeyframes {
        track: &'static str,
        index: usize,
        previous: u16,
        current: u16,
    },
    #[error("{field} contains a non-finite floating-point value")]
    NonFiniteFloat { field: &'static str },
    #[error("{field} has {count} items; maximum is {maximum}")]
    CountOverflow {
        field: &'static str,
        count: usize,
        maximum: usize,
    },
    #[error("effect component {field} has {actual} bytes; expected {expected}")]
    InvalidPropertyDataLength {
        field: &'static str,
        actual: usize,
        expected: usize,
    },
    #[error("unsupported effect component kind code {code}; expected 0, 1, or 2")]
    UnsupportedComponentKind { code: u8 },
    #[error(
        "transform animation {track} table offset {offset} is before the {minimum}-byte header"
    )]
    InvalidTableOffset {
        track: &'static str,
        offset: i32,
        minimum: usize,
    },
}

pub type EffectResult<T> = std::result::Result<T, EffectError>;

/// Supported effect payload family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectAssetKind {
    ColorAnimation,
    Definition,
    TransformAnimation,
    TextureAnimation,
}

/// One effect payload with its family retained in the type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "asset", rename_all = "kebab-case")]
pub enum EffectAsset {
    ColorAnimation(ColorAnimation),
    Definition(EffectDefinition),
    TransformAnimation(TransformAnimation),
    TextureAnimation(TextureAnimation),
}

impl EffectAsset {
    pub fn kind(&self) -> EffectAssetKind {
        match self {
            Self::ColorAnimation(_) => EffectAssetKind::ColorAnimation,
            Self::Definition(_) => EffectAssetKind::Definition,
            Self::TransformAnimation(_) => EffectAssetKind::TransformAnimation,
            Self::TextureAnimation(_) => EffectAssetKind::TextureAnimation,
        }
    }
}

/// Shared signed timing header used by effect animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationTiming {
    pub first_frame: i16,
    pub last_frame: i16,
    pub frame_rate: i16,
    pub keyframe_step: i16,
}

/// An RGBA color. The native payload stores its bytes in BGRA order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RgbaColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorKeyframe {
    pub time: u16,
    pub color: RgbaColor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorAnimation {
    pub timing: AnimationTiming,
    pub keyframes: Vec<ColorKeyframe>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureKeyframe {
    pub time: u16,
    pub texture_resource_key: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureAnimation {
    pub timing: AnimationTiming,
    pub keyframes: Vec<TextureKeyframe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vector2Keyframe {
    pub time: u16,
    pub value: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackedRotationKeyframe {
    pub time: u16,
    pub rotation: [i16; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vector3Keyframe {
    pub time: u16,
    pub value: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformAnimation {
    pub timing: AnimationTiming,
    pub translation_keyframes: Vec<Vector2Keyframe>,
    pub rotation_keyframes: Vec<PackedRotationKeyframe>,
    pub scale_keyframes: Vec<Vector3Keyframe>,
}

/// Full-precision quaternion key used by an effect-definition track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuaternionKeyframe {
    pub time: u16,
    pub rotation: [f32; 4],
}

/// Serialized track-descriptor workspace. The loader overwrites the two slots
/// with key/value array addresses in its working copy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EffectTrackLoaderWorkspace {
    pub reserved_01_03: [u8; 3],
    pub key_array_slot: i32,
    pub value_array_slot: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuaternionTrack {
    #[serde(default)]
    pub loader_workspace: EffectTrackLoaderWorkspace,
    pub keyframes: Vec<QuaternionKeyframe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vector3Track {
    #[serde(default)]
    pub loader_workspace: EffectTrackLoaderWorkspace,
    pub keyframes: Vec<Vector3Keyframe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorTrack {
    #[serde(default)]
    pub loader_workspace: EffectTrackLoaderWorkspace,
    pub keyframes: Vec<ColorKeyframe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureTrack {
    #[serde(default)]
    pub loader_workspace: EffectTrackLoaderWorkspace,
    pub keyframes: Vec<TextureKeyframe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectTransformTracks {
    pub rotation: QuaternionTrack,
    pub scale: Vector3Track,
    pub translation: Vector3Track,
}

/// Four unsigned timing values embedded in an effect component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectAnimationTiming {
    pub first_key: u16,
    pub last_key: u16,
    pub rate: u16,
    pub units_per_key: u16,
}

/// The custom 16-bit floating-point representation used by particle and
/// flying-effect properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackedFloat16(pub u16);

impl PackedFloat16 {
    /// Expand the stored sign, four-bit exponent, and eleven-bit mantissa to
    /// an IEEE-754 `f32`.
    pub fn to_f32(self) -> f32 {
        if self.0 == 0 {
            return 0.0;
        }
        let sign = u32::from((self.0 & 0x8000) >> 15);
        let exponent = i32::from((self.0 & 0x7800) >> 11) - 7 + 127;
        let mantissa = u32::from(self.0 & 0x07ff);
        f32::from_bits((sign << 31) | ((exponent as u32) << 23) | (mantissa << 12))
    }
}

/// Properties used by positioned or actor-attached components (`kind = 0`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionedEffectProperties {
    pub placement_mode: u8,
    pub source_height_scale: f32,
    pub update_source_height: u8,
    pub use_target_height: u8,
    pub target_height_scale: f32,
    pub reserved_23_47: Vec<u8>,
}

/// Properties used by travelling components (`kind = 1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlyingEffectProperties {
    pub placement_mode: u8,
    pub target_height_scale: f32,
    pub target_offset: [f32; 3],
    pub source_height_scale: f32,
    pub source_offset: [f32; 3],
    pub fade_out_distance: f32,
    pub travel_rate: f32,
    pub sine_height_scale: f32,
    pub rotate_sine_offset: u8,
    pub sine_offset_x: PackedFloat16,
}

/// Properties used by particle-emitting components (`kind = 2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleEffectProperties {
    pub flags: u8,
    pub spawn_offset: [PackedFloat16; 3],
    pub rotation: [i16; 4],
    pub axis_random_range: [PackedFloat16; 3],
    pub particle_size: [PackedFloat16; 3],
    pub axis_randomization: [u8; 3],
    pub rotation_randomization: [u8; 3],
    pub scale_randomization: [u8; 3],
    pub gravity_factor: u8,
    pub initial_spawn_count: u8,
    pub spawn_count_base: u8,
    pub spawn_count_random_range: u8,
    pub spawn_delay_base_milliseconds: u16,
    pub spawn_delay_random_range_milliseconds: u16,
    pub particle_lifetime_base_milliseconds: u16,
    pub particle_lifetime_random_range_milliseconds: u16,
}

/// Kind-specific bytes stored at component offsets `0x18..0x47`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "properties", rename_all = "kebab-case")]
pub enum EffectComponentVariant {
    Positioned(PositionedEffectProperties),
    Flying(FlyingEffectProperties),
    Particle(ParticleEffectProperties),
}

impl EffectComponentVariant {
    pub fn code(&self) -> u8 {
        match self {
            Self::Positioned(_) => 0,
            Self::Flying(_) => 1,
            Self::Particle(_) => 2,
        }
    }
}

/// Typed properties stored in the first `0x60` bytes of an effect component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectComponentProperties {
    pub reserved_00_03: [u8; 4],
    pub variant: EffectComponentVariant,
    pub orientation_mode: u8,
    pub timeline_mode: u8,
    pub record_timeline_value: u8,
    pub source_blend_factor: i32,
    pub destination_blend_factor: i32,
    pub geometry_resource_key: i32,
    pub callback_value: i32,
    pub animation_timing: EffectAnimationTiming,
    pub start_offset_milliseconds: i32,
    pub duration_or_loop_milliseconds: i32,
    pub loop_animation: u8,
    pub cursor_mode: u8,
    pub timing_mode: u8,
    pub enable_depth_state: u8,
    pub transform_animation_enabled: u8,
    pub base_scale: u8,
    pub reserved_5e: u8,
    pub texture_transform_animation_enabled: u8,
}

/// One effect component with typed serialized properties and animation tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectComponent {
    pub properties: EffectComponentProperties,
    pub transform: EffectTransformTracks,
    pub color: ColorTrack,
    pub texture: TextureTrack,
    pub texture_transform: EffectTransformTracks,
}

impl EffectComponent {
    /// Component-kind discriminator stored at offset `0x04`.
    pub fn kind_code(&self) -> u8 {
        self.properties.variant.code()
    }

    /// Geometry resource key stored at fixed offset `0x10`.
    pub fn geometry_resource_key(&self) -> i32 {
        self.properties.geometry_resource_key
    }

    pub fn keyframe_count(&self) -> usize {
        self.transform.rotation.keyframes.len()
            + self.transform.scale.keyframes.len()
            + self.transform.translation.keyframes.len()
            + self.color.keyframes.len()
            + self.texture.keyframes.len()
            + self.texture_transform.rotation.keyframes.len()
            + self.texture_transform.scale.keyframes.len()
            + self.texture_transform.translation.keyframes.len()
    }
}

/// Serialized root-header workspace. The loader overwrites every field while
/// constructing its cached representation of an effect definition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EffectDefinitionLoaderWorkspace {
    pub loaded_tick_slot: i32,
    pub source_record_slot: i32,
    pub child_records_slot: i32,
    pub reference_count_slot: i32,
    pub flags_slot: u16,
}

/// One effect definition with its serialized loader workspace retained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectDefinition {
    pub resource_key: i32,
    #[serde(default)]
    pub loader_workspace: EffectDefinitionLoaderWorkspace,
    pub components: Vec<EffectComponent>,
}

impl EffectDefinition {
    pub fn keyframe_count(&self) -> usize {
        self.components
            .iter()
            .map(EffectComponent::keyframe_count)
            .sum()
    }

    pub fn geometry_resource_keys(&self) -> Vec<i32> {
        let mut keys = self
            .components
            .iter()
            .map(EffectComponent::geometry_resource_key)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        keys.dedup();
        keys
    }
}

pub fn decode_effect_asset(kind: EffectAssetKind, data: &[u8]) -> EffectResult<EffectAsset> {
    match kind {
        EffectAssetKind::ColorAnimation => {
            decode_color_animation(data).map(EffectAsset::ColorAnimation)
        }
        EffectAssetKind::Definition => decode_effect_definition(data).map(EffectAsset::Definition),
        EffectAssetKind::TransformAnimation => {
            decode_transform_animation(data).map(EffectAsset::TransformAnimation)
        }
        EffectAssetKind::TextureAnimation => {
            decode_texture_animation(data).map(EffectAsset::TextureAnimation)
        }
    }
}

pub fn write_effect_asset_bytes(asset: &EffectAsset) -> EffectResult<Vec<u8>> {
    match asset {
        EffectAsset::ColorAnimation(value) => write_color_animation_bytes(value),
        EffectAsset::Definition(value) => write_effect_definition_bytes(value),
        EffectAsset::TransformAnimation(value) => write_transform_animation_bytes(value),
        EffectAsset::TextureAnimation(value) => write_texture_animation_bytes(value),
    }
}

pub fn decode_color_animation(data: &[u8]) -> EffectResult<ColorAnimation> {
    let mut reader = ByteReader::new(data);
    let timing = read_timing(&mut reader)?;
    validate_timing(timing)?;
    let count = usize::from(reader.read_u16_le("color_animation.keyframe_count")?);
    let mut keyframes = Vec::with_capacity(count);
    for _ in 0..count {
        let time = reader.read_u16_le("color_animation.keyframe.time")?;
        let blue = reader.read_u8("color_animation.keyframe.blue")?;
        let green = reader.read_u8("color_animation.keyframe.green")?;
        let red = reader.read_u8("color_animation.keyframe.red")?;
        let alpha = reader.read_u8("color_animation.keyframe.alpha")?;
        keyframes.push(ColorKeyframe {
            time,
            color: RgbaColor {
                red,
                green,
                blue,
                alpha,
            },
        });
    }
    finish(&reader, "color animation")?;
    validate_key_order("color animation", &keyframes, |key| key.time)?;
    Ok(ColorAnimation { timing, keyframes })
}

pub fn write_color_animation_bytes(animation: &ColorAnimation) -> EffectResult<Vec<u8>> {
    validate_timing(animation.timing)?;
    validate_count(
        "color animation keyframes",
        animation.keyframes.len(),
        u16::MAX as usize,
    )?;
    validate_key_order("color animation", &animation.keyframes, |key| key.time)?;
    let mut output = Vec::with_capacity(VALUE_ANIMATION_HEADER_LEN + animation.keyframes.len() * 6);
    write_timing(&mut output, animation.timing);
    push_u16(&mut output, animation.keyframes.len() as u16);
    for keyframe in &animation.keyframes {
        push_u16(&mut output, keyframe.time);
        output.extend_from_slice(&[
            keyframe.color.blue,
            keyframe.color.green,
            keyframe.color.red,
            keyframe.color.alpha,
        ]);
    }
    Ok(output)
}

pub fn decode_texture_animation(data: &[u8]) -> EffectResult<TextureAnimation> {
    let mut reader = ByteReader::new(data);
    let timing = read_timing(&mut reader)?;
    validate_timing(timing)?;
    let count = usize::from(reader.read_u16_le("texture_animation.keyframe_count")?);
    let mut keyframes = Vec::with_capacity(count);
    for _ in 0..count {
        keyframes.push(TextureKeyframe {
            time: reader.read_u16_le("texture_animation.keyframe.time")?,
            texture_resource_key: reader
                .read_i32_le("texture_animation.keyframe.texture_resource_key")?,
        });
    }
    finish(&reader, "texture animation")?;
    validate_key_order("texture animation", &keyframes, |key| key.time)?;
    Ok(TextureAnimation { timing, keyframes })
}

pub fn write_texture_animation_bytes(animation: &TextureAnimation) -> EffectResult<Vec<u8>> {
    validate_timing(animation.timing)?;
    validate_count(
        "texture animation keyframes",
        animation.keyframes.len(),
        u16::MAX as usize,
    )?;
    validate_key_order("texture animation", &animation.keyframes, |key| key.time)?;
    let mut output = Vec::with_capacity(VALUE_ANIMATION_HEADER_LEN + animation.keyframes.len() * 6);
    write_timing(&mut output, animation.timing);
    push_u16(&mut output, animation.keyframes.len() as u16);
    for keyframe in &animation.keyframes {
        push_u16(&mut output, keyframe.time);
        push_i32(&mut output, keyframe.texture_resource_key);
    }
    Ok(output)
}

pub fn decode_transform_animation(data: &[u8]) -> EffectResult<TransformAnimation> {
    let mut reader = ByteReader::new(data);
    let timing = read_timing(&mut reader)?;
    validate_timing(timing)?;
    let translation_offset = read_offset(&mut reader, "transform_animation.translation_offset")?;
    let rotation_offset = read_offset(&mut reader, "transform_animation.rotation_offset")?;
    let scale_offset = read_offset(&mut reader, "transform_animation.scale_offset")?;

    let translation_keyframes = read_vector2_table(data, translation_offset)?;
    let rotation_keyframes = read_packed_rotation_table(data, rotation_offset)?;
    let scale_keyframes = read_vector3_table(data, scale_offset, "transform_animation.scale")?;
    validate_key_order("transform translation", &translation_keyframes, |key| {
        key.time
    })?;
    validate_key_order("transform rotation", &rotation_keyframes, |key| key.time)?;
    validate_key_order("transform scale", &scale_keyframes, |key| key.time)?;
    validate_vec2_keys(&translation_keyframes, "transform translation")?;
    validate_vec3_keys(&scale_keyframes, "transform scale")?;
    Ok(TransformAnimation {
        timing,
        translation_keyframes,
        rotation_keyframes,
        scale_keyframes,
    })
}

pub fn write_transform_animation_bytes(animation: &TransformAnimation) -> EffectResult<Vec<u8>> {
    validate_transform_animation(animation)?;
    let translation_len = 2 + animation.translation_keyframes.len() * 10;
    let rotation_len = 2 + animation.rotation_keyframes.len() * 10;
    let translation_offset = TRANSFORM_ANIMATION_HEADER_LEN;
    let rotation_offset = translation_offset + translation_len;
    let scale_offset = rotation_offset + rotation_len;
    validate_count("transform payload offset", scale_offset, i32::MAX as usize)?;

    let mut output = Vec::new();
    write_timing(&mut output, animation.timing);
    push_i32(&mut output, translation_offset as i32);
    push_i32(&mut output, rotation_offset as i32);
    push_i32(&mut output, scale_offset as i32);
    write_vector2_table(&mut output, &animation.translation_keyframes);
    write_packed_rotation_table(&mut output, &animation.rotation_keyframes);
    write_vector3_table(&mut output, &animation.scale_keyframes);
    Ok(output)
}

pub fn decode_effect_definition(data: &[u8]) -> EffectResult<EffectDefinition> {
    let mut reader = ByteReader::new(data);
    let resource_key = reader.read_i32_le("effect_definition.resource_key")?;
    let loaded_tick_slot = reader.read_i32_le("effect_definition.loaded_tick_slot")?;
    let source_record_slot = reader.read_i32_le("effect_definition.source_record_slot")?;
    let component_count = usize::from(reader.read_u16_le("effect_definition.component_count")?);
    let loader_workspace = EffectDefinitionLoaderWorkspace {
        loaded_tick_slot,
        source_record_slot,
        child_records_slot: reader.read_i32_le("effect_definition.child_records_slot")?,
        reference_count_slot: reader.read_i32_le("effect_definition.reference_count_slot")?,
        flags_slot: reader.read_u16_le("effect_definition.flags_slot")?,
    };

    let fixed_bytes = component_count
        .checked_mul(EFFECT_COMPONENT_RECORD_LEN)
        .ok_or(EffectError::CountOverflow {
            field: "effect definition components",
            count: component_count,
            maximum: usize::MAX / EFFECT_COMPONENT_RECORD_LEN,
        })?;
    reader.read_bytes("effect_definition.component_records", fixed_bytes)?;

    let mut components = Vec::with_capacity(component_count);
    for component_index in 0..component_count {
        let start = EFFECT_DEFINITION_HEADER_LEN + component_index * EFFECT_COMPONENT_RECORD_LEN;
        let mut component_reader = ByteReader::new_at(data, start);
        let properties = read_component_properties(&mut component_reader)?;
        let transform_rotation = read_descriptor(&mut component_reader)?;
        let transform_scale = read_descriptor(&mut component_reader)?;
        let transform_translation = read_descriptor(&mut component_reader)?;
        let color = read_descriptor(&mut component_reader)?;
        let texture = read_descriptor(&mut component_reader)?;
        let texture_rotation = read_descriptor(&mut component_reader)?;
        let texture_scale = read_descriptor(&mut component_reader)?;
        let texture_translation = read_descriptor(&mut component_reader)?;

        components.push(PendingComponent {
            properties,
            transform_rotation,
            transform_scale,
            transform_translation,
            color,
            texture,
            texture_rotation,
            texture_scale,
            texture_translation,
        });
    }

    let mut data_reader = ByteReader::new_at(
        data,
        EFFECT_DEFINITION_HEADER_LEN + component_count * EFFECT_COMPONENT_RECORD_LEN,
    );
    let mut decoded_components = Vec::with_capacity(component_count);
    for pending in components {
        decoded_components.push(read_component_tracks(&mut data_reader, pending)?);
    }
    finish(&data_reader, "effect definition")?;
    let definition = EffectDefinition {
        resource_key,
        loader_workspace,
        components: decoded_components,
    };
    validate_effect_definition(&definition)?;
    Ok(definition)
}

pub fn write_effect_definition_bytes(definition: &EffectDefinition) -> EffectResult<Vec<u8>> {
    validate_effect_definition(definition)?;
    let mut output = Vec::new();
    push_i32(&mut output, definition.resource_key);
    push_i32(&mut output, definition.loader_workspace.loaded_tick_slot);
    push_i32(&mut output, definition.loader_workspace.source_record_slot);
    push_u16(&mut output, definition.components.len() as u16);
    push_i32(&mut output, definition.loader_workspace.child_records_slot);
    push_i32(
        &mut output,
        definition.loader_workspace.reference_count_slot,
    );
    push_u16(&mut output, definition.loader_workspace.flags_slot);
    for component in &definition.components {
        write_component_properties(&mut output, &component.properties);
        write_descriptor(&mut output, &component.transform.rotation);
        write_descriptor(&mut output, &component.transform.scale);
        write_descriptor(&mut output, &component.transform.translation);
        write_descriptor(&mut output, &component.color);
        write_descriptor(&mut output, &component.texture);
        write_descriptor(&mut output, &component.texture_transform.rotation);
        write_descriptor(&mut output, &component.texture_transform.scale);
        write_descriptor(&mut output, &component.texture_transform.translation);
    }
    for component in &definition.components {
        write_quaternion_effect_track(&mut output, &component.transform.rotation.keyframes);
        write_vector3_effect_track(&mut output, &component.transform.scale.keyframes);
        write_vector3_effect_track(&mut output, &component.transform.translation.keyframes);
        write_color_effect_track(&mut output, &component.color.keyframes);
        write_texture_effect_track(&mut output, &component.texture.keyframes);
        write_quaternion_effect_track(&mut output, &component.texture_transform.rotation.keyframes);
        write_vector3_effect_track(&mut output, &component.texture_transform.scale.keyframes);
        write_vector3_effect_track(
            &mut output,
            &component.texture_transform.translation.keyframes,
        );
    }
    Ok(output)
}

fn read_component_properties(
    reader: &mut ByteReader<'_>,
) -> EffectResult<EffectComponentProperties> {
    let reserved_00_03 = reader.read_array("effect_definition.component.reserved_00_03")?;
    let kind = reader.read_u8("effect_definition.component.kind")?;
    let orientation_mode = reader.read_u8("effect_definition.component.orientation_mode")?;
    let timeline_mode = reader.read_u8("effect_definition.component.timeline_mode")?;
    let record_timeline_value =
        reader.read_u8("effect_definition.component.record_timeline_value")?;
    let source_blend_factor =
        reader.read_i32_le("effect_definition.component.source_blend_factor")?;
    let destination_blend_factor =
        reader.read_i32_le("effect_definition.component.destination_blend_factor")?;
    let geometry_resource_key =
        reader.read_i32_le("effect_definition.component.geometry_resource_key")?;
    let callback_value = reader.read_i32_le("effect_definition.component.callback_value")?;
    let variant = read_component_variant(reader, kind)?;
    let animation_timing = EffectAnimationTiming {
        first_key: reader.read_u16_le("effect_definition.component.timing.first_key")?,
        last_key: reader.read_u16_le("effect_definition.component.timing.last_key")?,
        rate: reader.read_u16_le("effect_definition.component.timing.rate")?,
        units_per_key: reader.read_u16_le("effect_definition.component.timing.units_per_key")?,
    };
    Ok(EffectComponentProperties {
        reserved_00_03,
        variant,
        orientation_mode,
        timeline_mode,
        record_timeline_value,
        source_blend_factor,
        destination_blend_factor,
        geometry_resource_key,
        callback_value,
        animation_timing,
        start_offset_milliseconds: reader
            .read_i32_le("effect_definition.component.start_offset_milliseconds")?,
        duration_or_loop_milliseconds: reader
            .read_i32_le("effect_definition.component.duration_or_loop_milliseconds")?,
        loop_animation: reader.read_u8("effect_definition.component.loop_animation")?,
        cursor_mode: reader.read_u8("effect_definition.component.cursor_mode")?,
        timing_mode: reader.read_u8("effect_definition.component.timing_mode")?,
        enable_depth_state: reader.read_u8("effect_definition.component.enable_depth_state")?,
        transform_animation_enabled: reader
            .read_u8("effect_definition.component.transform_animation_enabled")?,
        base_scale: reader.read_u8("effect_definition.component.base_scale")?,
        reserved_5e: reader.read_u8("effect_definition.component.reserved_5e")?,
        texture_transform_animation_enabled: reader
            .read_u8("effect_definition.component.texture_transform_animation_enabled")?,
    })
}

fn read_component_variant(
    reader: &mut ByteReader<'_>,
    kind: u8,
) -> EffectResult<EffectComponentVariant> {
    match kind {
        0 => Ok(EffectComponentVariant::Positioned(
            PositionedEffectProperties {
                placement_mode: reader
                    .read_u8("effect_definition.component.positioned.placement_mode")?,
                source_height_scale: reader
                    .read_f32_le("effect_definition.component.positioned.source_height_scale")?,
                update_source_height: reader
                    .read_u8("effect_definition.component.positioned.update_source_height")?,
                use_target_height: reader
                    .read_u8("effect_definition.component.positioned.use_target_height")?,
                target_height_scale: reader
                    .read_f32_le("effect_definition.component.positioned.target_height_scale")?,
                reserved_23_47: reader
                    .read_bytes(
                        "effect_definition.component.positioned.reserved_23_47",
                        0x25,
                    )?
                    .to_vec(),
            },
        )),
        1 => Ok(EffectComponentVariant::Flying(FlyingEffectProperties {
            placement_mode: reader.read_u8("effect_definition.component.flying.placement_mode")?,
            target_height_scale: reader
                .read_f32_le("effect_definition.component.flying.target_height_scale")?,
            target_offset: read_vec3(reader, "effect_definition.component.flying.target_offset")?,
            source_height_scale: reader
                .read_f32_le("effect_definition.component.flying.source_height_scale")?,
            source_offset: read_vec3(reader, "effect_definition.component.flying.source_offset")?,
            fade_out_distance: reader
                .read_f32_le("effect_definition.component.flying.fade_out_distance")?,
            travel_rate: reader.read_f32_le("effect_definition.component.flying.travel_rate")?,
            sine_height_scale: reader
                .read_f32_le("effect_definition.component.flying.sine_height_scale")?,
            rotate_sine_offset: reader
                .read_u8("effect_definition.component.flying.rotate_sine_offset")?,
            sine_offset_x: read_packed_float16(
                reader,
                "effect_definition.component.flying.sine_offset_x",
            )?,
        })),
        2 => Ok(EffectComponentVariant::Particle(ParticleEffectProperties {
            flags: reader.read_u8("effect_definition.component.particle.flags")?,
            spawn_offset: read_packed_float16_vec3(
                reader,
                "effect_definition.component.particle.spawn_offset",
            )?,
            rotation: [
                reader.read_i16_le("effect_definition.component.particle.rotation.x")?,
                reader.read_i16_le("effect_definition.component.particle.rotation.y")?,
                reader.read_i16_le("effect_definition.component.particle.rotation.z")?,
                reader.read_i16_le("effect_definition.component.particle.rotation.w")?,
            ],
            axis_random_range: read_packed_float16_vec3(
                reader,
                "effect_definition.component.particle.axis_random_range",
            )?,
            particle_size: read_packed_float16_vec3(
                reader,
                "effect_definition.component.particle.particle_size",
            )?,
            axis_randomization: reader
                .read_array("effect_definition.component.particle.axis_randomization")?,
            rotation_randomization: reader
                .read_array("effect_definition.component.particle.rotation_randomization")?,
            scale_randomization: reader
                .read_array("effect_definition.component.particle.scale_randomization")?,
            gravity_factor: reader
                .read_u8("effect_definition.component.particle.gravity_factor")?,
            initial_spawn_count: reader
                .read_u8("effect_definition.component.particle.initial_spawn_count")?,
            spawn_count_base: reader
                .read_u8("effect_definition.component.particle.spawn_count_base")?,
            spawn_count_random_range: reader
                .read_u8("effect_definition.component.particle.spawn_count_random_range")?,
            spawn_delay_base_milliseconds: reader.read_u16_le(
                "effect_definition.component.particle.spawn_delay_base_milliseconds",
            )?,
            spawn_delay_random_range_milliseconds: reader.read_u16_le(
                "effect_definition.component.particle.spawn_delay_random_range_milliseconds",
            )?,
            particle_lifetime_base_milliseconds: reader.read_u16_le(
                "effect_definition.component.particle.particle_lifetime_base_milliseconds",
            )?,
            particle_lifetime_random_range_milliseconds: reader.read_u16_le(
                "effect_definition.component.particle.particle_lifetime_random_range_milliseconds",
            )?,
        })),
        code => Err(EffectError::UnsupportedComponentKind { code }),
    }
}

fn write_component_properties(output: &mut Vec<u8>, properties: &EffectComponentProperties) {
    output.extend_from_slice(&properties.reserved_00_03);
    output.push(properties.variant.code());
    output.push(properties.orientation_mode);
    output.push(properties.timeline_mode);
    output.push(properties.record_timeline_value);
    push_i32(output, properties.source_blend_factor);
    push_i32(output, properties.destination_blend_factor);
    push_i32(output, properties.geometry_resource_key);
    push_i32(output, properties.callback_value);
    write_component_variant(output, &properties.variant);
    push_u16(output, properties.animation_timing.first_key);
    push_u16(output, properties.animation_timing.last_key);
    push_u16(output, properties.animation_timing.rate);
    push_u16(output, properties.animation_timing.units_per_key);
    push_i32(output, properties.start_offset_milliseconds);
    push_i32(output, properties.duration_or_loop_milliseconds);
    output.extend_from_slice(&[
        properties.loop_animation,
        properties.cursor_mode,
        properties.timing_mode,
        properties.enable_depth_state,
        properties.transform_animation_enabled,
        properties.base_scale,
        properties.reserved_5e,
        properties.texture_transform_animation_enabled,
    ]);
}

fn write_component_variant(output: &mut Vec<u8>, variant: &EffectComponentVariant) {
    match variant {
        EffectComponentVariant::Positioned(properties) => {
            output.push(properties.placement_mode);
            push_f32(output, properties.source_height_scale);
            output.push(properties.update_source_height);
            output.push(properties.use_target_height);
            push_f32(output, properties.target_height_scale);
            output.extend_from_slice(&properties.reserved_23_47);
        }
        EffectComponentVariant::Flying(properties) => {
            output.push(properties.placement_mode);
            push_f32(output, properties.target_height_scale);
            write_vec3(output, properties.target_offset);
            push_f32(output, properties.source_height_scale);
            write_vec3(output, properties.source_offset);
            push_f32(output, properties.fade_out_distance);
            push_f32(output, properties.travel_rate);
            push_f32(output, properties.sine_height_scale);
            output.push(properties.rotate_sine_offset);
            push_u16(output, properties.sine_offset_x.0);
        }
        EffectComponentVariant::Particle(properties) => {
            output.push(properties.flags);
            write_packed_float16_vec3(output, properties.spawn_offset);
            for value in properties.rotation {
                push_i16(output, value);
            }
            write_packed_float16_vec3(output, properties.axis_random_range);
            write_packed_float16_vec3(output, properties.particle_size);
            output.extend_from_slice(&properties.axis_randomization);
            output.extend_from_slice(&properties.rotation_randomization);
            output.extend_from_slice(&properties.scale_randomization);
            output.extend_from_slice(&[
                properties.gravity_factor,
                properties.initial_spawn_count,
                properties.spawn_count_base,
                properties.spawn_count_random_range,
            ]);
            push_u16(output, properties.spawn_delay_base_milliseconds);
            push_u16(output, properties.spawn_delay_random_range_milliseconds);
            push_u16(output, properties.particle_lifetime_base_milliseconds);
            push_u16(
                output,
                properties.particle_lifetime_random_range_milliseconds,
            );
        }
    }
}

fn read_packed_float16(
    reader: &mut ByteReader<'_>,
    field: &'static str,
) -> EffectResult<PackedFloat16> {
    Ok(PackedFloat16(reader.read_u16_le(field)?))
}

fn read_packed_float16_vec3(
    reader: &mut ByteReader<'_>,
    field: &'static str,
) -> EffectResult<[PackedFloat16; 3]> {
    Ok([
        read_packed_float16(reader, field)?,
        read_packed_float16(reader, field)?,
        read_packed_float16(reader, field)?,
    ])
}

fn write_packed_float16_vec3(output: &mut Vec<u8>, values: [PackedFloat16; 3]) {
    for value in values {
        push_u16(output, value.0);
    }
}

#[derive(Debug)]
struct Descriptor {
    count: usize,
    loader_workspace: EffectTrackLoaderWorkspace,
}

#[derive(Debug)]
struct PendingComponent {
    properties: EffectComponentProperties,
    transform_rotation: Descriptor,
    transform_scale: Descriptor,
    transform_translation: Descriptor,
    color: Descriptor,
    texture: Descriptor,
    texture_rotation: Descriptor,
    texture_scale: Descriptor,
    texture_translation: Descriptor,
}

fn read_descriptor(reader: &mut ByteReader<'_>) -> EffectResult<Descriptor> {
    Ok(Descriptor {
        count: usize::from(reader.read_u8("effect_definition.track.count")?),
        loader_workspace: EffectTrackLoaderWorkspace {
            reserved_01_03: reader.read_array("effect_definition.track.reserved_01_03")?,
            key_array_slot: reader.read_i32_le("effect_definition.track.key_array_slot")?,
            value_array_slot: reader.read_i32_le("effect_definition.track.value_array_slot")?,
        },
    })
}

fn read_component_tracks(
    reader: &mut ByteReader<'_>,
    pending: PendingComponent,
) -> EffectResult<EffectComponent> {
    let transform_rotation = QuaternionTrack {
        loader_workspace: pending.transform_rotation.loader_workspace,
        keyframes: read_quaternion_effect_track(reader, pending.transform_rotation.count)?,
    };
    let transform_scale = Vector3Track {
        loader_workspace: pending.transform_scale.loader_workspace,
        keyframes: read_vector3_effect_track(reader, pending.transform_scale.count)?,
    };
    let transform_translation = Vector3Track {
        loader_workspace: pending.transform_translation.loader_workspace,
        keyframes: read_vector3_effect_track(reader, pending.transform_translation.count)?,
    };
    let color = ColorTrack {
        loader_workspace: pending.color.loader_workspace,
        keyframes: read_color_effect_track(reader, pending.color.count)?,
    };
    let texture = TextureTrack {
        loader_workspace: pending.texture.loader_workspace,
        keyframes: read_texture_effect_track(reader, pending.texture.count)?,
    };
    let texture_rotation = QuaternionTrack {
        loader_workspace: pending.texture_rotation.loader_workspace,
        keyframes: read_quaternion_effect_track(reader, pending.texture_rotation.count)?,
    };
    let texture_scale = Vector3Track {
        loader_workspace: pending.texture_scale.loader_workspace,
        keyframes: read_vector3_effect_track(reader, pending.texture_scale.count)?,
    };
    let texture_translation = Vector3Track {
        loader_workspace: pending.texture_translation.loader_workspace,
        keyframes: read_vector3_effect_track(reader, pending.texture_translation.count)?,
    };
    Ok(EffectComponent {
        properties: pending.properties,
        transform: EffectTransformTracks {
            rotation: transform_rotation,
            scale: transform_scale,
            translation: transform_translation,
        },
        color,
        texture,
        texture_transform: EffectTransformTracks {
            rotation: texture_rotation,
            scale: texture_scale,
            translation: texture_translation,
        },
    })
}

fn read_quaternion_effect_track(
    reader: &mut ByteReader<'_>,
    count: usize,
) -> EffectResult<Vec<QuaternionKeyframe>> {
    let times = read_times(reader, count)?;
    let mut values = Vec::with_capacity(count);
    for time in times {
        values.push(QuaternionKeyframe {
            time,
            rotation: [
                reader.read_f32_le("effect_definition.quaternion.x")?,
                reader.read_f32_le("effect_definition.quaternion.y")?,
                reader.read_f32_le("effect_definition.quaternion.z")?,
                reader.read_f32_le("effect_definition.quaternion.w")?,
            ],
        });
    }
    Ok(values)
}

fn read_vector3_effect_track(
    reader: &mut ByteReader<'_>,
    count: usize,
) -> EffectResult<Vec<Vector3Keyframe>> {
    let times = read_times(reader, count)?;
    let mut values = Vec::with_capacity(count);
    for time in times {
        values.push(Vector3Keyframe {
            time,
            value: read_vec3(reader, "effect_definition.vector")?,
        });
    }
    Ok(values)
}

fn read_color_effect_track(
    reader: &mut ByteReader<'_>,
    count: usize,
) -> EffectResult<Vec<ColorKeyframe>> {
    let times = read_times(reader, count)?;
    let mut values = Vec::with_capacity(count);
    for time in times {
        let blue = reader.read_u8("effect_definition.color.blue")?;
        let green = reader.read_u8("effect_definition.color.green")?;
        let red = reader.read_u8("effect_definition.color.red")?;
        let alpha = reader.read_u8("effect_definition.color.alpha")?;
        values.push(ColorKeyframe {
            time,
            color: RgbaColor {
                red,
                green,
                blue,
                alpha,
            },
        });
    }
    Ok(values)
}

fn read_texture_effect_track(
    reader: &mut ByteReader<'_>,
    count: usize,
) -> EffectResult<Vec<TextureKeyframe>> {
    let times = read_times(reader, count)?;
    let mut values = Vec::with_capacity(count);
    for time in times {
        values.push(TextureKeyframe {
            time,
            texture_resource_key: reader.read_i32_le("effect_definition.texture.resource_key")?,
        });
    }
    Ok(values)
}

fn read_times(reader: &mut ByteReader<'_>, count: usize) -> EffectResult<Vec<u16>> {
    let mut times = Vec::with_capacity(count);
    for _ in 0..count {
        times.push(reader.read_u16_le("effect_definition.track.time")?);
    }
    Ok(times)
}

trait TrackDescriptor {
    fn count(&self) -> usize;
    fn loader_workspace(&self) -> EffectTrackLoaderWorkspace;
}

macro_rules! impl_track_descriptor {
    ($type:ty) => {
        impl TrackDescriptor for $type {
            fn count(&self) -> usize {
                self.keyframes.len()
            }
            fn loader_workspace(&self) -> EffectTrackLoaderWorkspace {
                self.loader_workspace
            }
        }
    };
}

impl_track_descriptor!(QuaternionTrack);
impl_track_descriptor!(Vector3Track);
impl_track_descriptor!(ColorTrack);
impl_track_descriptor!(TextureTrack);

fn write_descriptor(output: &mut Vec<u8>, track: &impl TrackDescriptor) {
    output.push(track.count() as u8);
    let workspace = track.loader_workspace();
    output.extend_from_slice(&workspace.reserved_01_03);
    push_i32(output, workspace.key_array_slot);
    push_i32(output, workspace.value_array_slot);
}

fn write_quaternion_effect_track(output: &mut Vec<u8>, keyframes: &[QuaternionKeyframe]) {
    write_times(output, keyframes.iter().map(|key| key.time));
    for keyframe in keyframes {
        for component in keyframe.rotation {
            push_f32(output, component);
        }
    }
}

fn write_vector3_effect_track(output: &mut Vec<u8>, keyframes: &[Vector3Keyframe]) {
    write_times(output, keyframes.iter().map(|key| key.time));
    for keyframe in keyframes {
        write_vec3(output, keyframe.value);
    }
}

fn write_color_effect_track(output: &mut Vec<u8>, keyframes: &[ColorKeyframe]) {
    write_times(output, keyframes.iter().map(|key| key.time));
    for keyframe in keyframes {
        output.extend_from_slice(&[
            keyframe.color.blue,
            keyframe.color.green,
            keyframe.color.red,
            keyframe.color.alpha,
        ]);
    }
}

fn write_texture_effect_track(output: &mut Vec<u8>, keyframes: &[TextureKeyframe]) {
    write_times(output, keyframes.iter().map(|key| key.time));
    for keyframe in keyframes {
        push_i32(output, keyframe.texture_resource_key);
    }
}

fn write_times(output: &mut Vec<u8>, times: impl IntoIterator<Item = u16>) {
    for time in times {
        push_u16(output, time);
    }
}

fn validate_effect_definition(definition: &EffectDefinition) -> EffectResult<()> {
    validate_count(
        "effect definition components",
        definition.components.len(),
        u16::MAX as usize,
    )?;
    for component in &definition.components {
        validate_component_properties(&component.properties)?;
        validate_track(
            "effect transform rotation",
            &component.transform.rotation.keyframes,
            |key| key.time,
        )?;
        validate_track(
            "effect transform scale",
            &component.transform.scale.keyframes,
            |key| key.time,
        )?;
        validate_track(
            "effect transform translation",
            &component.transform.translation.keyframes,
            |key| key.time,
        )?;
        validate_track("effect color", &component.color.keyframes, |key| key.time)?;
        validate_track("effect texture", &component.texture.keyframes, |key| {
            key.time
        })?;
        validate_track(
            "effect texture rotation",
            &component.texture_transform.rotation.keyframes,
            |key| key.time,
        )?;
        validate_track(
            "effect texture scale",
            &component.texture_transform.scale.keyframes,
            |key| key.time,
        )?;
        validate_track(
            "effect texture translation",
            &component.texture_transform.translation.keyframes,
            |key| key.time,
        )?;
        validate_quaternion_keys(
            &component.transform.rotation.keyframes,
            "effect transform rotation",
        )?;
        validate_vec3_keys(
            &component.transform.scale.keyframes,
            "effect transform scale",
        )?;
        validate_vec3_keys(
            &component.transform.translation.keyframes,
            "effect transform translation",
        )?;
        validate_quaternion_keys(
            &component.texture_transform.rotation.keyframes,
            "effect texture rotation",
        )?;
        validate_vec3_keys(
            &component.texture_transform.scale.keyframes,
            "effect texture scale",
        )?;
        validate_vec3_keys(
            &component.texture_transform.translation.keyframes,
            "effect texture translation",
        )?;
    }
    Ok(())
}

fn validate_component_properties(properties: &EffectComponentProperties) -> EffectResult<()> {
    match &properties.variant {
        EffectComponentVariant::Positioned(positioned) => {
            validate_property_data_length(
                "positioned reserved_23_47",
                positioned.reserved_23_47.len(),
                0x25,
            )?;
            validate_floats(
                "effect positioned height scales",
                &[
                    positioned.source_height_scale,
                    positioned.target_height_scale,
                ],
            )?;
        }
        EffectComponentVariant::Flying(flying) => {
            validate_floats(
                "effect flying properties",
                &[
                    flying.target_height_scale,
                    flying.target_offset[0],
                    flying.target_offset[1],
                    flying.target_offset[2],
                    flying.source_height_scale,
                    flying.source_offset[0],
                    flying.source_offset[1],
                    flying.source_offset[2],
                    flying.fade_out_distance,
                    flying.travel_rate,
                    flying.sine_height_scale,
                ],
            )?;
        }
        EffectComponentVariant::Particle(_) => {}
    }
    Ok(())
}

fn validate_property_data_length(
    field: &'static str,
    actual: usize,
    expected: usize,
) -> EffectResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(EffectError::InvalidPropertyDataLength {
            field,
            actual,
            expected,
        })
    }
}

fn validate_track<T>(
    name: &'static str,
    keyframes: &[T],
    time: impl Fn(&T) -> u16,
) -> EffectResult<()> {
    validate_count(name, keyframes.len(), u8::MAX as usize)?;
    validate_key_order(name, keyframes, time)
}

fn validate_transform_animation(animation: &TransformAnimation) -> EffectResult<()> {
    validate_timing(animation.timing)?;
    validate_count(
        "transform translation keyframes",
        animation.translation_keyframes.len(),
        u16::MAX as usize,
    )?;
    validate_count(
        "transform rotation keyframes",
        animation.rotation_keyframes.len(),
        u16::MAX as usize,
    )?;
    validate_count(
        "transform scale keyframes",
        animation.scale_keyframes.len(),
        u16::MAX as usize,
    )?;
    validate_key_order(
        "transform translation",
        &animation.translation_keyframes,
        |key| key.time,
    )?;
    validate_key_order("transform rotation", &animation.rotation_keyframes, |key| {
        key.time
    })?;
    validate_key_order("transform scale", &animation.scale_keyframes, |key| {
        key.time
    })?;
    validate_vec2_keys(&animation.translation_keyframes, "transform translation")?;
    validate_vec3_keys(&animation.scale_keyframes, "transform scale")
}

fn validate_timing(timing: AnimationTiming) -> EffectResult<()> {
    if timing.first_frame < 0
        || timing.first_frame > timing.last_frame
        || timing.frame_rate <= 0
        || timing.keyframe_step <= 0
    {
        return Err(EffectError::InvalidTiming {
            first_frame: timing.first_frame,
            last_frame: timing.last_frame,
            frame_rate: timing.frame_rate,
            keyframe_step: timing.keyframe_step,
        });
    }
    Ok(())
}

fn validate_key_order<T>(
    track: &'static str,
    keyframes: &[T],
    time: impl Fn(&T) -> u16,
) -> EffectResult<()> {
    for (index, pair) in keyframes.windows(2).enumerate() {
        let previous = time(&pair[0]);
        let current = time(&pair[1]);
        if previous >= current {
            return Err(EffectError::UnorderedKeyframes {
                track,
                index: index + 1,
                previous,
                current,
            });
        }
    }
    Ok(())
}

fn validate_vec2_keys(keyframes: &[Vector2Keyframe], field: &'static str) -> EffectResult<()> {
    for keyframe in keyframes {
        validate_floats(field, &keyframe.value)?;
    }
    Ok(())
}

fn validate_vec3_keys(keyframes: &[Vector3Keyframe], field: &'static str) -> EffectResult<()> {
    for keyframe in keyframes {
        validate_floats(field, &keyframe.value)?;
    }
    Ok(())
}

fn validate_quaternion_keys(
    keyframes: &[QuaternionKeyframe],
    field: &'static str,
) -> EffectResult<()> {
    for keyframe in keyframes {
        validate_floats(field, &keyframe.rotation)?;
    }
    Ok(())
}

fn validate_floats(field: &'static str, values: &[f32]) -> EffectResult<()> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(EffectError::NonFiniteFloat { field });
    }
    Ok(())
}

fn validate_count(field: &'static str, count: usize, maximum: usize) -> EffectResult<()> {
    if count > maximum {
        Err(EffectError::CountOverflow {
            field,
            count,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn read_timing(reader: &mut ByteReader<'_>) -> EffectResult<AnimationTiming> {
    Ok(AnimationTiming {
        first_frame: reader.read_i16_le("animation.first_frame")?,
        last_frame: reader.read_i16_le("animation.last_frame")?,
        frame_rate: reader.read_i16_le("animation.frame_rate")?,
        keyframe_step: reader.read_i16_le("animation.keyframe_step")?,
    })
}

fn read_offset(reader: &mut ByteReader<'_>, field: &'static str) -> EffectResult<usize> {
    let offset = reader.read_i32_le(field)?;
    if offset < TRANSFORM_ANIMATION_HEADER_LEN as i32 {
        return Err(EffectError::InvalidTableOffset {
            track: field,
            offset,
            minimum: TRANSFORM_ANIMATION_HEADER_LEN,
        });
    }
    Ok(offset as usize)
}

fn read_vector2_table(data: &[u8], offset: usize) -> EffectResult<Vec<Vector2Keyframe>> {
    let mut reader = ByteReader::new_at(data, offset);
    let count = usize::from(reader.read_u16_le("transform_animation.translation.count")?);
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        keys.push(Vector2Keyframe {
            time: reader.read_u16_le("transform_animation.translation.time")?,
            value: [
                reader.read_f32_le("transform_animation.translation.x")?,
                reader.read_f32_le("transform_animation.translation.y")?,
            ],
        });
    }
    Ok(keys)
}

fn read_packed_rotation_table(
    data: &[u8],
    offset: usize,
) -> EffectResult<Vec<PackedRotationKeyframe>> {
    let mut reader = ByteReader::new_at(data, offset);
    let count = usize::from(reader.read_u16_le("transform_animation.rotation.count")?);
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        keys.push(PackedRotationKeyframe {
            time: reader.read_u16_le("transform_animation.rotation.time")?,
            rotation: [
                reader.read_i16_le("transform_animation.rotation.x")?,
                reader.read_i16_le("transform_animation.rotation.y")?,
                reader.read_i16_le("transform_animation.rotation.z")?,
                reader.read_i16_le("transform_animation.rotation.w")?,
            ],
        });
    }
    Ok(keys)
}

fn read_vector3_table(
    data: &[u8],
    offset: usize,
    field: &'static str,
) -> EffectResult<Vec<Vector3Keyframe>> {
    let mut reader = ByteReader::new_at(data, offset);
    let count = usize::from(reader.read_u16_le(field)?);
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        keys.push(Vector3Keyframe {
            time: reader.read_u16_le("transform_animation.vector.time")?,
            value: read_vec3(&mut reader, field)?,
        });
    }
    Ok(keys)
}

fn finish(reader: &ByteReader<'_>, asset: &'static str) -> EffectResult<()> {
    if reader.remaining() > 0 {
        Err(EffectError::TrailingBytes {
            asset,
            count: reader.remaining(),
        })
    } else {
        Ok(())
    }
}

fn read_vec3(reader: &mut ByteReader<'_>, field: &'static str) -> EffectResult<[f32; 3]> {
    Ok([
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
    ])
}

fn write_timing(output: &mut Vec<u8>, timing: AnimationTiming) {
    push_i16(output, timing.first_frame);
    push_i16(output, timing.last_frame);
    push_i16(output, timing.frame_rate);
    push_i16(output, timing.keyframe_step);
}

fn write_vector2_table(output: &mut Vec<u8>, keyframes: &[Vector2Keyframe]) {
    push_u16(output, keyframes.len() as u16);
    for keyframe in keyframes {
        push_u16(output, keyframe.time);
        push_f32(output, keyframe.value[0]);
        push_f32(output, keyframe.value[1]);
    }
}

fn write_packed_rotation_table(output: &mut Vec<u8>, keyframes: &[PackedRotationKeyframe]) {
    push_u16(output, keyframes.len() as u16);
    for keyframe in keyframes {
        push_u16(output, keyframe.time);
        for component in keyframe.rotation {
            push_i16(output, component);
        }
    }
}

fn write_vector3_table(output: &mut Vec<u8>, keyframes: &[Vector3Keyframe]) {
    push_u16(output, keyframes.len() as u16);
    for keyframe in keyframes {
        push_u16(output, keyframe.time);
        write_vec3(output, keyframe.value);
    }
}

fn write_vec3(output: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        push_f32(output, component);
    }
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_i16(output: &mut Vec<u8>, value: i16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(output: &mut Vec<u8>, value: f32) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing() -> AnimationTiming {
        AnimationTiming {
            first_frame: 0,
            last_frame: 30,
            frame_rate: 30,
            keyframe_step: 160,
        }
    }

    fn workspace(value: u8) -> EffectTrackLoaderWorkspace {
        EffectTrackLoaderWorkspace {
            reserved_01_03: [value; 3],
            key_array_slot: 0x1000 + i32::from(value),
            value_array_slot: 0x2000 + i32::from(value),
        }
    }

    fn empty_transform(value: u8) -> EffectTransformTracks {
        EffectTransformTracks {
            rotation: QuaternionTrack {
                loader_workspace: workspace(value),
                keyframes: Vec::new(),
            },
            scale: Vector3Track {
                loader_workspace: workspace(value + 1),
                keyframes: Vec::new(),
            },
            translation: Vector3Track {
                loader_workspace: workspace(value + 2),
                keyframes: Vec::new(),
            },
        }
    }

    fn component_properties(variant: EffectComponentVariant) -> EffectComponentProperties {
        EffectComponentProperties {
            reserved_00_03: [0x5a; 4],
            variant,
            orientation_mode: 0x83,
            timeline_mode: 1,
            record_timeline_value: 1,
            source_blend_factor: 0x0302,
            destination_blend_factor: 0x0303,
            geometry_resource_key: 123,
            callback_value: 456,
            animation_timing: EffectAnimationTiming {
                first_key: 0,
                last_key: 10,
                rate: 30,
                units_per_key: 160,
            },
            start_offset_milliseconds: 25,
            duration_or_loop_milliseconds: 1_000,
            loop_animation: 1,
            cursor_mode: 2,
            timing_mode: 3,
            enable_depth_state: 1,
            transform_animation_enabled: 1,
            base_scale: 2,
            reserved_5e: 0x5e,
            texture_transform_animation_enabled: 1,
        }
    }

    #[test]
    fn color_animation_round_trips_bgra_layout() {
        let animation = ColorAnimation {
            timing: timing(),
            keyframes: vec![ColorKeyframe {
                time: 7,
                color: RgbaColor {
                    red: 1,
                    green: 2,
                    blue: 3,
                    alpha: 4,
                },
            }],
        };
        let bytes = write_color_animation_bytes(&animation).unwrap();
        assert_eq!(&bytes[12..16], &[3, 2, 1, 4]);
        assert_eq!(decode_color_animation(&bytes).unwrap(), animation);
    }

    #[test]
    fn texture_animation_round_trips() {
        let animation = TextureAnimation {
            timing: timing(),
            keyframes: vec![
                TextureKeyframe {
                    time: 0,
                    texture_resource_key: 0x4f00_0001,
                },
                TextureKeyframe {
                    time: 160,
                    texture_resource_key: -1,
                },
            ],
        };
        let bytes = write_texture_animation_bytes(&animation).unwrap();
        assert_eq!(decode_texture_animation(&bytes).unwrap(), animation);
    }

    #[test]
    fn transform_animation_round_trips_all_tracks() {
        let animation = TransformAnimation {
            timing: timing(),
            translation_keyframes: vec![Vector2Keyframe {
                time: 0,
                value: [1.0, 2.0],
            }],
            rotation_keyframes: vec![PackedRotationKeyframe {
                time: 1,
                rotation: [0, 0, 0, 32767],
            }],
            scale_keyframes: vec![Vector3Keyframe {
                time: 2,
                value: [1.0, 2.0, 3.0],
            }],
        };
        let bytes = write_transform_animation_bytes(&animation).unwrap();
        assert_eq!(i32::from_le_bytes(bytes[8..12].try_into().unwrap()), 20);
        assert_eq!(decode_transform_animation(&bytes).unwrap(), animation);
    }

    #[test]
    fn transform_animation_accepts_independent_table_offsets() {
        let expected = TransformAnimation {
            timing: timing(),
            translation_keyframes: vec![Vector2Keyframe {
                time: 10,
                value: [1.0, 2.0],
            }],
            rotation_keyframes: Vec::new(),
            scale_keyframes: vec![Vector3Keyframe {
                time: 20,
                value: [3.0, 4.0, 5.0],
            }],
        };
        let mut translation_table = Vec::new();
        write_vector2_table(&mut translation_table, &expected.translation_keyframes);
        let mut rotation_table = Vec::new();
        write_packed_rotation_table(&mut rotation_table, &expected.rotation_keyframes);
        let mut scale_table = Vec::new();
        write_vector3_table(&mut scale_table, &expected.scale_keyframes);

        let scale_offset = TRANSFORM_ANIMATION_HEADER_LEN + 3;
        let translation_offset = scale_offset + scale_table.len() + 2;
        let rotation_offset = translation_offset + translation_table.len();
        let mut bytes = Vec::new();
        write_timing(&mut bytes, expected.timing);
        push_i32(&mut bytes, translation_offset as i32);
        push_i32(&mut bytes, rotation_offset as i32);
        push_i32(&mut bytes, scale_offset as i32);
        bytes.extend_from_slice(&[0xaa; 3]);
        bytes.extend_from_slice(&scale_table);
        bytes.extend_from_slice(&[0xbb; 2]);
        bytes.extend_from_slice(&translation_table);
        bytes.extend_from_slice(&rotation_table);
        bytes.extend_from_slice(&[0xcc; 4]);

        assert_eq!(decode_transform_animation(&bytes).unwrap(), expected);
    }

    #[test]
    fn transform_animation_accepts_shared_empty_table() {
        let mut bytes = Vec::new();
        write_timing(&mut bytes, timing());
        for _ in 0..3 {
            push_i32(&mut bytes, TRANSFORM_ANIMATION_HEADER_LEN as i32);
        }
        push_u16(&mut bytes, 0);

        assert_eq!(
            decode_transform_animation(&bytes).unwrap(),
            TransformAnimation {
                timing: timing(),
                translation_keyframes: Vec::new(),
                rotation_keyframes: Vec::new(),
                scale_keyframes: Vec::new(),
            }
        );
    }

    #[test]
    fn transform_animation_rejects_invalid_references_and_values() {
        let animation = TransformAnimation {
            timing: timing(),
            translation_keyframes: vec![Vector2Keyframe {
                time: 0,
                value: [f32::NAN, 0.0],
            }],
            rotation_keyframes: Vec::new(),
            scale_keyframes: Vec::new(),
        };
        assert!(matches!(
            write_transform_animation_bytes(&animation),
            Err(EffectError::NonFiniteFloat { .. })
        ));

        let mut bytes = write_transform_animation_bytes(&TransformAnimation {
            translation_keyframes: Vec::new(),
            ..animation
        })
        .unwrap();
        bytes[8..12].copy_from_slice(&19_i32.to_le_bytes());
        assert!(matches!(
            decode_transform_animation(&bytes),
            Err(EffectError::InvalidTableOffset { offset: 19, .. })
        ));

        let mut truncated = write_transform_animation_bytes(&TransformAnimation {
            timing: timing(),
            translation_keyframes: Vec::new(),
            rotation_keyframes: Vec::new(),
            scale_keyframes: vec![Vector3Keyframe {
                time: 0,
                value: [1.0, 1.0, 1.0],
            }],
        })
        .unwrap();
        truncated.pop();
        assert!(matches!(
            decode_transform_animation(&truncated),
            Err(EffectError::Truncated(_))
        ));
    }

    #[test]
    fn component_property_variants_round_trip_exactly() {
        let variants = vec![
            EffectComponentVariant::Positioned(PositionedEffectProperties {
                placement_mode: 2,
                source_height_scale: 0.5,
                update_source_height: 1,
                use_target_height: 1,
                target_height_scale: 0.75,
                reserved_23_47: vec![0x23; 0x25],
            }),
            EffectComponentVariant::Flying(FlyingEffectProperties {
                placement_mode: 4,
                target_height_scale: 0.25,
                target_offset: [1.0, 2.0, 3.0],
                source_height_scale: 0.5,
                source_offset: [4.0, 5.0, 6.0],
                fade_out_distance: 7.0,
                travel_rate: 8.0,
                sine_height_scale: 9.0,
                rotate_sine_offset: 1,
                sine_offset_x: PackedFloat16(0xb800),
            }),
            EffectComponentVariant::Particle(ParticleEffectProperties {
                flags: 0x80,
                spawn_offset: [
                    PackedFloat16(0x3800),
                    PackedFloat16(0),
                    PackedFloat16(0xb800),
                ],
                rotation: [1, 2, 3, 4],
                axis_random_range: [
                    PackedFloat16(0x4000),
                    PackedFloat16(0x4400),
                    PackedFloat16(0x4800),
                ],
                particle_size: [
                    PackedFloat16(0x3800),
                    PackedFloat16(0x3800),
                    PackedFloat16(0x3800),
                ],
                axis_randomization: [2, 4, 6],
                rotation_randomization: [3, 5, 7],
                scale_randomization: [8, 9, 10],
                gravity_factor: 11,
                initial_spawn_count: 12,
                spawn_count_base: 13,
                spawn_count_random_range: 14,
                spawn_delay_base_milliseconds: 0x1516,
                spawn_delay_random_range_milliseconds: 0x1718,
                particle_lifetime_base_milliseconds: 0x191a,
                particle_lifetime_random_range_milliseconds: 0x1b1c,
            }),
        ];

        for variant in variants {
            let expected = component_properties(variant);
            validate_component_properties(&expected).unwrap();
            let mut bytes = Vec::new();
            write_component_properties(&mut bytes, &expected);
            assert_eq!(bytes.len(), EFFECT_COMPONENT_FIXED_DATA_LEN);
            assert_eq!(bytes[4], expected.variant.code());
            assert_eq!(bytes[5], expected.orientation_mode);
            assert_eq!(
                i32::from_le_bytes(bytes[0x10..0x14].try_into().unwrap()),
                expected.geometry_resource_key
            );
            assert_eq!(
                i32::from_le_bytes(bytes[0x14..0x18].try_into().unwrap()),
                expected.callback_value
            );
            assert_eq!(
                u16::from_le_bytes(bytes[0x48..0x4a].try_into().unwrap()),
                expected.animation_timing.first_key
            );
            assert_eq!(bytes[0x5d], expected.base_scale);
            assert_eq!(bytes[0x5f], expected.texture_transform_animation_enabled);
            if let EffectComponentVariant::Particle(properties) = &expected.variant {
                assert_eq!(bytes[0x36..0x39], properties.rotation_randomization);
                assert_eq!(bytes[0x39..0x3c], properties.scale_randomization);
                assert_eq!(bytes[0x3c], properties.gravity_factor);
                assert_eq!(bytes[0x3d], properties.initial_spawn_count);
                assert_eq!(bytes[0x3e], properties.spawn_count_base);
                assert_eq!(bytes[0x3f], properties.spawn_count_random_range);
                assert_eq!(
                    u16::from_le_bytes(bytes[0x40..0x42].try_into().unwrap()),
                    properties.spawn_delay_base_milliseconds
                );
                assert_eq!(
                    u16::from_le_bytes(bytes[0x42..0x44].try_into().unwrap()),
                    properties.spawn_delay_random_range_milliseconds
                );
                assert_eq!(
                    u16::from_le_bytes(bytes[0x44..0x46].try_into().unwrap()),
                    properties.particle_lifetime_base_milliseconds
                );
                assert_eq!(
                    u16::from_le_bytes(bytes[0x46..0x48].try_into().unwrap()),
                    properties.particle_lifetime_random_range_milliseconds
                );
            }
            let mut reader = ByteReader::new(&bytes);
            assert_eq!(read_component_properties(&mut reader).unwrap(), expected);
            assert_eq!(reader.remaining(), 0);
        }

        assert_eq!(PackedFloat16(0).to_f32(), 0.0);
        assert_eq!(PackedFloat16(0x3800).to_f32(), 1.0);
        assert_eq!(PackedFloat16(0xb800).to_f32(), -1.0);
    }

    #[test]
    fn effect_definition_round_trips_every_track_and_loader_workspace() {
        let component = EffectComponent {
            properties: component_properties(EffectComponentVariant::Particle(
                ParticleEffectProperties {
                    flags: 0x81,
                    spawn_offset: [
                        PackedFloat16(0x3800),
                        PackedFloat16(0),
                        PackedFloat16(0xb800),
                    ],
                    rotation: [0, 0, 0, 32767],
                    axis_random_range: [
                        PackedFloat16(0x4000),
                        PackedFloat16(0x4400),
                        PackedFloat16(0x4800),
                    ],
                    particle_size: [
                        PackedFloat16(0x3800),
                        PackedFloat16(0x3800),
                        PackedFloat16(0x3800),
                    ],
                    axis_randomization: [2, 4, 6],
                    rotation_randomization: [3, 5, 7],
                    scale_randomization: [8, 9, 10],
                    gravity_factor: 11,
                    initial_spawn_count: 12,
                    spawn_count_base: 13,
                    spawn_count_random_range: 14,
                    spawn_delay_base_milliseconds: 15,
                    spawn_delay_random_range_milliseconds: 16,
                    particle_lifetime_base_milliseconds: 17,
                    particle_lifetime_random_range_milliseconds: 18,
                },
            )),
            transform: EffectTransformTracks {
                rotation: QuaternionTrack {
                    loader_workspace: workspace(1),
                    keyframes: vec![QuaternionKeyframe {
                        time: 1,
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    }],
                },
                scale: Vector3Track {
                    loader_workspace: workspace(2),
                    keyframes: vec![Vector3Keyframe {
                        time: 2,
                        value: [1.0, 1.0, 1.0],
                    }],
                },
                translation: Vector3Track {
                    loader_workspace: workspace(3),
                    keyframes: vec![Vector3Keyframe {
                        time: 3,
                        value: [1.0, 2.0, 3.0],
                    }],
                },
            },
            color: ColorTrack {
                loader_workspace: workspace(4),
                keyframes: vec![ColorKeyframe {
                    time: 4,
                    color: RgbaColor {
                        red: 10,
                        green: 20,
                        blue: 30,
                        alpha: 40,
                    },
                }],
            },
            texture: TextureTrack {
                loader_workspace: workspace(5),
                keyframes: vec![TextureKeyframe {
                    time: 5,
                    texture_resource_key: 55,
                }],
            },
            texture_transform: EffectTransformTracks {
                rotation: QuaternionTrack {
                    loader_workspace: workspace(6),
                    keyframes: vec![QuaternionKeyframe {
                        time: 6,
                        rotation: [1.0, 0.0, 0.0, 0.0],
                    }],
                },
                scale: Vector3Track {
                    loader_workspace: workspace(7),
                    keyframes: vec![Vector3Keyframe {
                        time: 7,
                        value: [2.0, 2.0, 2.0],
                    }],
                },
                translation: Vector3Track {
                    loader_workspace: workspace(8),
                    keyframes: vec![Vector3Keyframe {
                        time: 8,
                        value: [4.0, 5.0, 6.0],
                    }],
                },
            },
        };
        let definition = EffectDefinition {
            resource_key: 99,
            loader_workspace: EffectDefinitionLoaderWorkspace {
                loaded_tick_slot: 0x1111_1111,
                source_record_slot: 0x2222_2222,
                child_records_slot: 0x3333_3333,
                reference_count_slot: 0x4444_4444,
                flags_slot: 0x5555,
            },
            components: vec![component],
        };
        let bytes = write_effect_definition_bytes(&definition).unwrap();
        assert_eq!(bytes.len(), 24 + 192 + 104);
        assert_eq!(
            i32::from_le_bytes(bytes[0x04..0x08].try_into().unwrap()),
            0x1111_1111
        );
        assert_eq!(
            i32::from_le_bytes(bytes[0x08..0x0c].try_into().unwrap()),
            0x2222_2222
        );
        assert_eq!(u16::from_le_bytes(bytes[0x0c..0x0e].try_into().unwrap()), 1);
        assert_eq!(
            i32::from_le_bytes(bytes[0x0e..0x12].try_into().unwrap()),
            0x3333_3333
        );
        assert_eq!(
            i32::from_le_bytes(bytes[0x12..0x16].try_into().unwrap()),
            0x4444_4444
        );
        assert_eq!(
            u16::from_le_bytes(bytes[0x16..0x18].try_into().unwrap()),
            0x5555
        );

        let rotation_descriptor = EFFECT_DEFINITION_HEADER_LEN + EFFECT_COMPONENT_FIXED_DATA_LEN;
        assert_eq!(bytes[rotation_descriptor], 1);
        assert_eq!(
            bytes[rotation_descriptor + 1..rotation_descriptor + 4],
            [1; 3]
        );
        assert_eq!(
            i32::from_le_bytes(
                bytes[rotation_descriptor + 4..rotation_descriptor + 8]
                    .try_into()
                    .unwrap()
            ),
            0x1001
        );
        assert_eq!(
            i32::from_le_bytes(
                bytes[rotation_descriptor + 8..rotation_descriptor + 12]
                    .try_into()
                    .unwrap()
            ),
            0x2001
        );

        let decoded = decode_effect_definition(&bytes).unwrap();
        assert_eq!(decoded, definition);
        assert_eq!(write_effect_definition_bytes(&decoded).unwrap(), bytes);
    }

    #[test]
    fn effect_definition_rejects_unsupported_component_kinds() {
        for code in [3, u8::MAX] {
            let mut bytes = vec![0; EFFECT_DEFINITION_HEADER_LEN + EFFECT_COMPONENT_RECORD_LEN];
            bytes[0x0c..0x0e].copy_from_slice(&1_u16.to_le_bytes());
            bytes[EFFECT_DEFINITION_HEADER_LEN + 0x04] = code;

            assert!(matches!(
                decode_effect_definition(&bytes),
                Err(EffectError::UnsupportedComponentKind { code: actual }) if actual == code
            ));
        }
    }

    #[test]
    fn effect_definition_rejects_bad_property_data_and_trailing_bytes() {
        let definition = EffectDefinition {
            resource_key: 1,
            loader_workspace: EffectDefinitionLoaderWorkspace {
                loaded_tick_slot: 0,
                source_record_slot: 0,
                child_records_slot: 0,
                reference_count_slot: 0,
                flags_slot: 0,
            },
            components: vec![EffectComponent {
                properties: component_properties(EffectComponentVariant::Positioned(
                    PositionedEffectProperties {
                        placement_mode: 0,
                        source_height_scale: 1.0,
                        update_source_height: 0,
                        use_target_height: 0,
                        target_height_scale: 1.0,
                        reserved_23_47: vec![0; 0x24],
                    },
                )),
                transform: empty_transform(0),
                color: ColorTrack {
                    loader_workspace: workspace(3),
                    keyframes: Vec::new(),
                },
                texture: TextureTrack {
                    loader_workspace: workspace(4),
                    keyframes: Vec::new(),
                },
                texture_transform: empty_transform(5),
            }],
        };
        assert!(write_effect_definition_bytes(&definition).is_err());

        let mut empty = write_effect_definition_bytes(&EffectDefinition {
            components: Vec::new(),
            ..definition
        })
        .unwrap();
        empty.push(0);
        assert!(matches!(
            decode_effect_definition(&empty),
            Err(EffectError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn unordered_keys_and_invalid_timing_are_rejected() {
        let animation = TextureAnimation {
            timing: AnimationTiming {
                frame_rate: 0,
                ..timing()
            },
            keyframes: Vec::new(),
        };
        assert!(matches!(
            write_texture_animation_bytes(&animation),
            Err(EffectError::InvalidTiming { .. })
        ));

        let animation = ColorAnimation {
            timing: timing(),
            keyframes: vec![
                ColorKeyframe {
                    time: 4,
                    color: RgbaColor {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: 0,
                    },
                },
                ColorKeyframe {
                    time: 4,
                    color: RgbaColor {
                        red: 1,
                        green: 1,
                        blue: 1,
                        alpha: 1,
                    },
                },
            ],
        };
        assert!(matches!(
            write_color_animation_bytes(&animation),
            Err(EffectError::UnorderedKeyframes { .. })
        ));
    }
}
