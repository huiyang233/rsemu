use std::fs;
use std::path::Path;

use crate::memory::{FirmwareImage, FirmwareSegment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareFormat {
    Bin,
    Elf,
}

pub struct FirmwareLoader;

impl FirmwareLoader {
    pub fn load_file(
        path: impl AsRef<Path>,
        default_bin_address: u64,
    ) -> Result<FirmwareImage, String> {
        let path = path.as_ref();
        let bytes = fs::read(path)
            .map_err(|err| format!("failed to read firmware file {}: {err}", path.display()))?;
        let format = detect_format(path, &bytes);

        match format {
            FirmwareFormat::Bin => Ok(FirmwareImage::from_bin(default_bin_address, &bytes)),
            FirmwareFormat::Elf => parse_elf32(&bytes),
        }
    }
}

fn detect_format(path: &Path, bytes: &[u8]) -> FirmwareFormat {
    if bytes.len() >= 4 && &bytes[0..4] == b"\x7FELF" {
        return FirmwareFormat::Elf;
    }

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("elf") => FirmwareFormat::Elf,
        _ => FirmwareFormat::Bin,
    }
}

fn parse_elf32(bytes: &[u8]) -> Result<FirmwareImage, String> {
    if bytes.len() < 52 {
        return Err("ELF file is too small".to_string());
    }
    if &bytes[0..4] != b"\x7FELF" {
        return Err("invalid ELF magic".to_string());
    }
    if bytes[4] != 1 {
        return Err("only ELF32 is supported".to_string());
    }
    if bytes[5] != 1 {
        return Err("only little-endian ELF is supported".to_string());
    }

    let program_header_offset = read_u32(bytes, 28)? as usize;
    let program_header_entry_size = read_u16(bytes, 42)? as usize;
    let program_header_count = read_u16(bytes, 44)? as usize;

    if program_header_entry_size < 32 {
        return Err("unexpected ELF program header size".to_string());
    }

    let mut segments = Vec::new();

    for index in 0..program_header_count {
        let base = program_header_offset + index * program_header_entry_size;
        if base + 32 > bytes.len() {
            return Err("ELF program header exceeds file length".to_string());
        }

        let p_type = read_u32(bytes, base)?;
        if p_type != 1 {
            continue;
        }

        let file_offset = read_u32(bytes, base + 4)? as usize;
        let physical_address = read_u32(bytes, base + 12)? as u64;
        let file_size = read_u32(bytes, base + 16)? as usize;
        let memory_size = read_u32(bytes, base + 20)? as usize;

        if file_size == 0 && memory_size == 0 {
            continue;
        }
        if file_offset + file_size > bytes.len() {
            return Err("ELF load segment exceeds file length".to_string());
        }

        let mut data = vec![0u8; memory_size.max(file_size)];
        if file_size > 0 {
            data[..file_size].copy_from_slice(&bytes[file_offset..file_offset + file_size]);
        }
        segments.push(FirmwareSegment {
            load_address: physical_address,
            bytes: data,
        });
    }

    if segments.is_empty() {
        return Err("ELF file has no PT_LOAD segment".to_string());
    }

    Ok(FirmwareImage::from_segments(segments))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let end = offset + 2;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| format!("ELF read out of range at 0x{offset:x}"))?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset + 4;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| format!("ELF read out of range at 0x{offset:x}"))?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}
