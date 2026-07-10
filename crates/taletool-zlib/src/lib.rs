//! zlib 1.1.2 compression support for NosTale archive writing.
//!
//! NosTale shipped archives were produced with old zlib output that modern
//! Rust zlib wrappers do not necessarily reproduce byte-for-byte. This crate
//! keeps that compatibility boundary in one place by compiling a vendored
//! zlib 1.1.2 encoder and exposing a small profile API for archive writers and
//! archive presets.
//!
//! Profile strings use the CLI-facing form
//! `zlib112-level<N>-<strategy>`, for example
//! `zlib112-level1-default` or `zlib112-level9-huffman`.
//! Supported levels are `0..=9`; supported strategies are `default`,
//! `filtered`, and `huffman`.

use std::fmt;
use std::os::raw::{c_int, c_uchar, c_ulong};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A zlib 1.1.2 compression profile.
///
/// Profiles are archive-wide writer settings: individual archive entries may
/// still be stored as raw or zlib data, but zlib entries are encoded using this
/// level and strategy.
///
/// Use [`ZlibProfile::default_level`] for the common NosTale presets, or parse
/// a CLI-style profile string:
///
/// ```
/// use taletool_zlib::{ZlibProfile, ZlibStrategy};
///
/// let profile: ZlibProfile = "zlib112-level9-default".parse().unwrap();
/// assert_eq!(profile.level, 9);
/// assert_eq!(profile.strategy, ZlibStrategy::Default);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ZlibProfile {
    /// zlib compression level, `0..=9`.
    pub level: u8,
    /// zlib 1.1.2 compression strategy.
    pub strategy: ZlibStrategy,
}

/// Compression strategies used with zlib 1.1.2 profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZlibStrategy {
    /// zlib `Z_DEFAULT_STRATEGY`.
    Default,
    /// zlib `Z_FILTERED`.
    Filtered,
    /// zlib `Z_HUFFMAN_ONLY`.
    Huffman,
}

/// Return every zlib112 profile combination supported by this crate.
///
/// The list contains all supported levels (`0..=9`) crossed with the supported
/// strategies (`default`, `filtered`, `huffman`).
pub fn candidate_profiles() -> Vec<ZlibProfile> {
    let strategies = [
        ZlibStrategy::Default,
        ZlibStrategy::Filtered,
        ZlibStrategy::Huffman,
    ];
    (0..=9)
        .flat_map(|level| {
            strategies
                .iter()
                .copied()
                .map(move |strategy| ZlibProfile { level, strategy })
        })
        .collect()
}

/// Compress bytes with the vendored zlib 1.1.2 encoder and the given profile.
///
/// This returns a complete zlib stream, not a raw DEFLATE stream. The output is
/// deterministic for the same input and profile, which is important for
/// reproducing shipped NosTale archive payloads.
///
/// # Errors
///
/// Returns an error if the profile level is outside `0..=9`, if the input or
/// output size cannot be represented for the C zlib 1.1.2 ABI, or if zlib
/// reports a compression failure.
pub fn compress_zlib112_profile(data: &[u8], profile: ZlibProfile) -> anyhow::Result<Vec<u8>> {
    if profile.level > 9 {
        anyhow::bail!("zlib 1.1.2 profile level must be in 0..=9");
    }
    let source_len = c_ulong::try_from(data.len())
        .map_err(|_| anyhow::anyhow!("input is too large for zlib 1.1.2"))?;
    let bound = unsafe { taletool_zlib112_compress_bound(source_len) };
    let mut output = vec![
        0_u8;
        usize::try_from(bound).map_err(|_| anyhow::anyhow!(
            "zlib 1.1.2 output bound is too large"
        ))?
    ];
    let mut output_len = bound;
    let status = unsafe {
        taletool_zlib112_compress(
            output.as_mut_ptr(),
            &mut output_len,
            data.as_ptr(),
            source_len,
            c_int::from(profile.level),
            profile.strategy.as_zlib112_strategy(),
        )
    };
    if status != 0 {
        anyhow::bail!("zlib 1.1.2 compression failed with status {status}");
    }
    output.truncate(
        usize::try_from(output_len)
            .map_err(|_| anyhow::anyhow!("zlib 1.1.2 output length is too large"))?,
    );
    Ok(output)
}

unsafe extern "C" {
    fn taletool_zlib112_compress_bound(source_len: c_ulong) -> c_ulong;

    fn taletool_zlib112_compress(
        dest: *mut c_uchar,
        dest_len: *mut c_ulong,
        source: *const c_uchar,
        source_len: c_ulong,
        level: c_int,
        strategy: c_int,
    ) -> c_int;
}

impl ZlibProfile {
    /// Construct a profile using zlib's default strategy at the given level.
    ///
    /// This does not validate `level`; validation happens when compression is
    /// attempted or when parsing from a string. Known NosTale presets use
    /// levels `1` and `9`.
    pub const fn default_level(level: u8) -> Self {
        Self {
            level,
            strategy: ZlibStrategy::Default,
        }
    }

    /// Ordering key used by profile detection tie-breaks.
    ///
    /// Prefer default strategy first, then filtered, then huffman; within a
    /// strategy, prefer higher compression levels.
    pub fn cmp_key(self) -> (u8, u8) {
        (self.strategy.cmp_key(), u8::MAX - self.level)
    }
}

impl ZlibStrategy {
    fn as_zlib112_strategy(self) -> c_int {
        match self {
            ZlibStrategy::Default => 0,
            ZlibStrategy::Filtered => 1,
            ZlibStrategy::Huffman => 2,
        }
    }

    fn cmp_key(self) -> u8 {
        match self {
            ZlibStrategy::Default => 0,
            ZlibStrategy::Filtered => 1,
            ZlibStrategy::Huffman => 2,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            ZlibStrategy::Default => "default",
            ZlibStrategy::Filtered => "filtered",
            ZlibStrategy::Huffman => "huffman",
        }
    }
}

impl fmt::Display for ZlibProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "zlib112-level{}-{}",
            self.level,
            self.strategy.as_str()
        )
    }
}

impl FromStr for ZlibProfile {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let rest = value
            .strip_prefix("zlib112-level")
            .ok_or_else(|| anyhow::anyhow!("zlib profile must start with zlib112-level"))?;
        let (level, strategy) = rest
            .split_once('-')
            .ok_or_else(|| anyhow::anyhow!("zlib profile must include a strategy"))?;
        let level = level.parse::<u8>()?;
        if level > 9 {
            anyhow::bail!("zlib profile level is outside the supported range");
        }
        let strategy = match strategy {
            "default" => ZlibStrategy::Default,
            "filtered" => ZlibStrategy::Filtered,
            "huffman" => ZlibStrategy::Huffman,
            _ => anyhow::bail!("unknown zlib 1.1.2 profile strategy: {strategy}"),
        };
        Ok(Self { level, strategy })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_parses_zlib112_profiles() {
        let profile = ZlibProfile {
            level: 1,
            strategy: ZlibStrategy::Default,
        };

        assert_eq!(profile.to_string(), "zlib112-level1-default");
        assert_eq!(
            "zlib112-level1-default".parse::<ZlibProfile>().unwrap(),
            profile
        );
        assert!("miniz-level9-fixed".parse::<ZlibProfile>().is_err());
        assert!("zlib112-level10-default".parse::<ZlibProfile>().is_err());
        assert!("zlib112-level1-fixed".parse::<ZlibProfile>().is_err());
        assert!("level9-default".parse::<ZlibProfile>().is_err());
    }

    #[test]
    fn compresses_deterministically_for_profiles() {
        let data = b"NosTale NosTale NosTale profile test";
        for profile in [
            ZlibProfile::default_level(1),
            ZlibProfile {
                level: 9,
                strategy: ZlibStrategy::Filtered,
            },
            ZlibProfile {
                level: 9,
                strategy: ZlibStrategy::Huffman,
            },
        ] {
            let encoded = compress_zlib112_profile(data, profile).unwrap();
            assert_eq!(encoded, compress_zlib112_profile(data, profile).unwrap());
        }
    }
}
