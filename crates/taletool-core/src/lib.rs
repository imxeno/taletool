use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub i32);

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

impl Diagnostic {
    pub fn info(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProgressEvent {
    Started { operation: String },
    Message { message: String },
    Finished { operation: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub archive: PathBuf,
    pub file_id: i32,
}

#[derive(Debug, Error)]
pub enum DataPathError {
    #[error("NosTale data directory does not exist: {0}")]
    MissingDataDir(PathBuf),
    #[error("path is not a directory: {0}")]
    NotDirectory(PathBuf),
}

pub fn validate_data_dir(path: impl Into<PathBuf>) -> Result<PathBuf, DataPathError> {
    let path = path.into();
    if !path.exists() {
        return Err(DataPathError::MissingDataDir(path));
    }
    if !path.is_dir() {
        return Err(DataPathError::NotDirectory(path));
    }
    Ok(path)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("field {field} is truncated at offset {offset}: need {needed} bytes, got {actual}")]
pub struct ByteReadError {
    pub field: &'static str,
    pub offset: usize,
    pub needed: usize,
    pub actual: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ByteReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    pub fn new_at(data: &'a [u8], offset: usize) -> Self {
        Self { data, offset }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    pub fn skip(&mut self, field: &'static str, len: usize) -> Result<(), ByteReadError> {
        self.ensure(field, len)?;
        self.offset += len;
        Ok(())
    }

    pub fn read_u8(&mut self, field: &'static str) -> Result<u8, ByteReadError> {
        Ok(self.read_array::<1>(field)?[0])
    }

    pub fn read_i8(&mut self, field: &'static str) -> Result<i8, ByteReadError> {
        Ok(self.read_u8(field)? as i8)
    }

    pub fn read_u16_le(&mut self, field: &'static str) -> Result<u16, ByteReadError> {
        Ok(u16::from_le_bytes(self.read_array(field)?))
    }

    pub fn read_i16_le(&mut self, field: &'static str) -> Result<i16, ByteReadError> {
        Ok(i16::from_le_bytes(self.read_array(field)?))
    }

    pub fn read_u32_le(&mut self, field: &'static str) -> Result<u32, ByteReadError> {
        Ok(u32::from_le_bytes(self.read_array(field)?))
    }

    pub fn read_u64_le(&mut self, field: &'static str) -> Result<u64, ByteReadError> {
        Ok(u64::from_le_bytes(self.read_array(field)?))
    }

    pub fn read_i32_le(&mut self, field: &'static str) -> Result<i32, ByteReadError> {
        Ok(i32::from_le_bytes(self.read_array(field)?))
    }

    pub fn read_f32_le(&mut self, field: &'static str) -> Result<f32, ByteReadError> {
        Ok(f32::from_le_bytes(self.read_array(field)?))
    }

    pub fn read_bytes(
        &mut self,
        field: &'static str,
        len: usize,
    ) -> Result<&'a [u8], ByteReadError> {
        self.ensure(field, len)?;
        let start = self.offset;
        self.offset += len;
        Ok(&self.data[start..start + len])
    }

    pub fn read_array<const N: usize>(
        &mut self,
        field: &'static str,
    ) -> Result<[u8; N], ByteReadError> {
        let bytes = self.read_bytes(field, N)?;
        let mut value = [0_u8; N];
        value.copy_from_slice(bytes);
        Ok(value)
    }

    fn ensure(&self, field: &'static str, len: usize) -> Result<(), ByteReadError> {
        if self.offset.saturating_add(len) > self.data.len() {
            return Err(ByteReadError {
                field,
                offset: self.offset,
                needed: len,
                actual: self.data.len().saturating_sub(self.offset),
            });
        }
        Ok(())
    }
}
