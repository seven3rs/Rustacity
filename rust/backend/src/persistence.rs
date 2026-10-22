// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The on-disk sidecar for a Rust-backed sequence.
//!
//! A `.rsq` file is deliberately small and versioned: a fixed header followed
//! by little-endian `f32` samples.  Version 1 has a 32-byte header and no
//! metadata; version 2 has a 64-byte header that also carries the sequence
//! extent (`min`, `max`, `rms`) and the sample format, so the C++
//! `RustBlockFile` can describe a clip without decoding its samples.  The
//! version-1 reader stays, because a sidecar is immutable once a saved project
//! refers to it.
//!
//! The C++ `RustBlockFile` owns the file name and the `DirManager` lifecycle;
//! this module only writes and reads the bytes so the header stays in one
//! place.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::Sequence;

const MAGIC: &[u8; 8] = b"RUSTSEQ1";
const VERSION_1: u32 = 1;
const VERSION_2: u32 = 2;
const CURRENT_VERSION: u32 = VERSION_2;
const CHANNELS: u32 = 1;
const FLOAT_FORMAT: u32 = 1;
const SAMPLE_FORMAT_FLOAT: u32 = 1;
const HEADER_V1_LEN: usize = 32;
const HEADER_V2_LEN: usize = 64;
const COPY_CHUNK: usize = 64 * 1024;
const TEMP_SUFFIX: &str = ".tmp";

/// The clip record stored beside the samples.
///
/// Version 1 of the file has none of this; version 2 computes it when the
/// sequence is saved and returns it from [`load_meta`] without reading the
/// samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipMeta {
    pub frames: u64,
    pub sample_rate: u32,
    pub sample_format: u32,
    pub min: f32,
    pub max: f32,
    pub rms: f32,
}

impl ClipMeta {
    fn from_sequence(sequence: &Sequence) -> Self {
        let frames = sequence.frames();
        let (min, max) = sequence.min_max(0, frames);
        Self {
            frames,
            sample_rate: sequence.sample_rate(),
            sample_format: SAMPLE_FORMAT_FLOAT,
            min,
            max,
            rms: sequence.rms(0, frames),
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(TEMP_SUFFIX);
    PathBuf::from(name)
}

/// Write `sequence` to `path` with the current versioned header.
///
/// The data is written to a sibling temporary file first, so a failed save
/// cannot truncate a sidecar that an already-saved project still references.
/// The temporary file is renamed into place only after every sample is flushed.
pub fn save(sequence: &Sequence, path: &Path) -> io::Result<()> {
    let meta = ClipMeta::from_sequence(sequence);
    if meta.sample_rate == 0 {
        return Err(invalid("a sequence needs a sample rate"));
    }

    let temporary = temp_path(path);
    let result = write_sequence(sequence, &temporary, meta);
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temporary, path) {
        // Leave the destination untouched: a failed save must not destroy a
        // sidecar that an already-saved project still references.
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    Ok(())
}

fn write_sequence(sequence: &Sequence, path: &Path, meta: ClipMeta) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);

    let mut header = [0_u8; HEADER_V2_LEN];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&CURRENT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&meta.sample_rate.to_le_bytes());
    header[16..20].copy_from_slice(&CHANNELS.to_le_bytes());
    header[20..24].copy_from_slice(&FLOAT_FORMAT.to_le_bytes());
    header[24..32].copy_from_slice(&meta.frames.to_le_bytes());
    header[32..36].copy_from_slice(&meta.min.to_le_bytes());
    header[36..40].copy_from_slice(&meta.max.to_le_bytes());
    header[40..44].copy_from_slice(&meta.rms.to_le_bytes());
    header[44..48].copy_from_slice(&meta.sample_format.to_le_bytes());
    // Everything after `sample_format` is reserved and stays zero, so a later
    // version can add fields without moving the samples.
    writer.write_all(&header)?;

    let mut samples = vec![0.0_f32; COPY_CHUNK];
    let mut bytes = Vec::with_capacity(COPY_CHUNK * 4);
    let mut position = 0_u64;
    while position < meta.frames {
        let count = usize::try_from((meta.frames - position).min(COPY_CHUNK as u64))
            .map_err(|_| invalid("sequence is too long for this platform"))?;
        let got = sequence.read(position, count, &mut samples[..count]);
        if got != count {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "sequence returned fewer samples than it reported",
            ));
        }

        bytes.clear();
        for sample in &samples[..got] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        writer.write_all(&bytes)?;
        position += got as u64;
    }

    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

/// Read a `.rsq` file into a new sequence.
pub fn load(path: &Path) -> io::Result<Sequence> {
    let file_len = fs::metadata(path)?.len();
    if file_len < HEADER_V1_LEN as u64 {
        return Err(invalid("the sequence file is shorter than its header"));
    }

    let mut file = File::open(path)?;
    let mut first = [0_u8; HEADER_V1_LEN];
    file.read_exact(&mut first)?;

    if &first[..8] != MAGIC {
        return Err(invalid("the sequence file has an unknown signature"));
    }

    match u32::from_le_bytes(first[8..12].try_into().expect("four-byte field")) {
        VERSION_1 => load_version_1(file, file_len, &first),
        VERSION_2 => load_version_2(file, file_len, &first),
        _ => Err(invalid("the sequence file has an unsupported version")),
    }
}

/// Read the metadata section without decoding the samples.
///
/// Returns [`io::ErrorKind::Unsupported`] for a version-1 file, so a caller
/// that still wants the clip can fall back to [`load`].
pub fn load_meta(path: &Path) -> io::Result<ClipMeta> {
    let file_len = fs::metadata(path)?.len();
    if file_len < HEADER_V1_LEN as u64 {
        return Err(invalid("the sequence file is shorter than its header"));
    }

    let mut file = File::open(path)?;
    let mut first = [0_u8; HEADER_V1_LEN];
    file.read_exact(&mut first)?;

    if &first[..8] != MAGIC {
        return Err(invalid("the sequence file has an unknown signature"));
    }

    match u32::from_le_bytes(first[8..12].try_into().expect("four-byte field")) {
        VERSION_1 => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the sequence file has no metadata section; it is version 1",
        )),
        VERSION_2 => {
            if file_len < HEADER_V2_LEN as u64 {
                return Err(invalid("the sequence file is shorter than its v2 header"));
            }
            let mut rest = [0_u8; HEADER_V2_LEN - HEADER_V1_LEN];
            file.read_exact(&mut rest)?;
            let mut header = [0_u8; HEADER_V2_LEN];
            header[..HEADER_V1_LEN].copy_from_slice(&first);
            header[HEADER_V1_LEN..].copy_from_slice(&rest);

            let meta = parse_version_2_header(&header)?;
            if meta.frames.checked_mul(4) != Some(file_len - HEADER_V2_LEN as u64) {
                return Err(invalid(
                    "the sequence file length does not match its frame count",
                ));
            }
            Ok(meta)
        }
        _ => Err(invalid("the sequence file has an unsupported version")),
    }
}

fn load_version_1(file: File, file_len: u64, header: &[u8; HEADER_V1_LEN]) -> io::Result<Sequence> {
    let (sample_rate, frames) = parse_version_1_header(header)?;
    if frames.checked_mul(4) != Some(file_len - HEADER_V1_LEN as u64) {
        return Err(invalid(
            "the sequence file length does not match its frame count",
        ));
    }
    read_samples(file, frames, sample_rate)
}

fn load_version_2(file: File, file_len: u64, first: &[u8; HEADER_V1_LEN]) -> io::Result<Sequence> {
    if file_len < HEADER_V2_LEN as u64 {
        return Err(invalid("the sequence file is shorter than its v2 header"));
    }
    let mut rest = [0_u8; HEADER_V2_LEN - HEADER_V1_LEN];
    let mut file = file;
    file.read_exact(&mut rest)?;
    let mut header = [0_u8; HEADER_V2_LEN];
    header[..HEADER_V1_LEN].copy_from_slice(first);
    header[HEADER_V1_LEN..].copy_from_slice(&rest);

    let meta = parse_version_2_header(&header)?;
    if meta.frames.checked_mul(4) != Some(file_len - HEADER_V2_LEN as u64) {
        return Err(invalid(
            "the sequence file length does not match its frame count",
        ));
    }
    read_samples(file, meta.frames, meta.sample_rate)
}

fn parse_version_1_header(header: &[u8; HEADER_V1_LEN]) -> io::Result<(u32, u64)> {
    let version = u32::from_le_bytes(header[8..12].try_into().expect("four-byte field"));
    if version != VERSION_1 {
        return Err(invalid("the sequence file has an unsupported version"));
    }
    let sample_rate = u32::from_le_bytes(header[12..16].try_into().expect("four-byte field"));
    if sample_rate == 0 {
        return Err(invalid("the sequence file has no sample rate"));
    }
    let channels = u32::from_le_bytes(header[16..20].try_into().expect("four-byte field"));
    let format = u32::from_le_bytes(header[20..24].try_into().expect("four-byte field"));
    if channels != CHANNELS || format != FLOAT_FORMAT {
        return Err(invalid("the sequence file is not mono float audio"));
    }
    let frames = u64::from_le_bytes(header[24..32].try_into().expect("eight-byte field"));
    Ok((sample_rate, frames))
}

fn parse_version_2_header(header: &[u8; HEADER_V2_LEN]) -> io::Result<ClipMeta> {
    let version = u32::from_le_bytes(header[8..12].try_into().expect("four-byte field"));
    if version != VERSION_2 {
        return Err(invalid("the sequence file has an unsupported version"));
    }
    let sample_rate = u32::from_le_bytes(header[12..16].try_into().expect("four-byte field"));
    if sample_rate == 0 {
        return Err(invalid("the sequence file has no sample rate"));
    }
    let channels = u32::from_le_bytes(header[16..20].try_into().expect("four-byte field"));
    let format = u32::from_le_bytes(header[20..24].try_into().expect("four-byte field"));
    if channels != CHANNELS || format != FLOAT_FORMAT {
        return Err(invalid("the sequence file is not mono float audio"));
    }

    let frames = u64::from_le_bytes(header[24..32].try_into().expect("eight-byte field"));
    let min = f32::from_le_bytes(header[32..36].try_into().expect("four-byte field"));
    let max = f32::from_le_bytes(header[36..40].try_into().expect("four-byte field"));
    let rms = f32::from_le_bytes(header[40..44].try_into().expect("four-byte field"));
    let sample_format = u32::from_le_bytes(header[44..48].try_into().expect("four-byte field"));

    if sample_format != SAMPLE_FORMAT_FLOAT {
        return Err(invalid(
            "the sequence metadata names an unknown sample format",
        ));
    }
    if !min.is_finite() || !max.is_finite() || !rms.is_finite() {
        return Err(invalid("the sequence metadata has a non-finite extent"));
    }
    if rms < 0.0 || min > max {
        return Err(invalid("the sequence metadata has an invalid extent"));
    }

    Ok(ClipMeta {
        frames,
        sample_rate,
        sample_format,
        min,
        max,
        rms,
    })
}

fn read_samples(file: File, frames: u64, sample_rate: u32) -> io::Result<Sequence> {
    let frames = usize::try_from(frames)
        .map_err(|_| invalid("the sequence is too long for this platform"))?;
    let byte_len = frames
        .checked_mul(4)
        .ok_or_else(|| invalid("the sequence is too long for this platform"))?;

    let mut file = file;
    let mut bytes = vec![0_u8; byte_len];
    file.read_exact(&mut bytes)?;
    let (chunks, remainder) = bytes.as_chunks::<4>();
    debug_assert!(remainder.is_empty());
    let samples = chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();

    Sequence::from_samples(sample_rate, samples)
        .map_err(|error| invalid(&format!("the sequence file is invalid: {error}")))
}

/// Build a silent sequence for callers that need an empty clip.
pub fn silence(frames: usize, sample_rate: u32) -> io::Result<Sequence> {
    Sequence::silent(frames, sample_rate)
        .map_err(|error| invalid(&format!("cannot create a silent sequence: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rustacity-persistence-{}-{stamp}-{label}.rsq",
            std::process::id()
        ))
    }

    fn version_1_bytes(sample_rate: u32, samples: &[f32]) -> Vec<u8> {
        let mut bytes = vec![0_u8; HEADER_V1_LEN];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&VERSION_1.to_le_bytes());
        bytes[12..16].copy_from_slice(&sample_rate.to_le_bytes());
        bytes[16..20].copy_from_slice(&CHANNELS.to_le_bytes());
        bytes[20..24].copy_from_slice(&FLOAT_FORMAT.to_le_bytes());
        bytes[24..32].copy_from_slice(&(samples.len() as u64).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn version_2_round_trips_samples_and_exposes_metadata() {
        let path = temp_path("v2");
        let sequence = Sequence::from_samples(8_000, vec![0.0, 1.0, -1.0, 0.5]).expect("sequence");

        save(&sequence, &path).expect("save");
        let meta = load_meta(&path).expect("metadata");
        assert_eq!(meta.frames, 4);
        assert_eq!(meta.sample_rate, 8_000);
        assert_eq!(meta.sample_format, SAMPLE_FORMAT_FLOAT);
        assert!((meta.min + 1.0).abs() < 1e-6);
        assert!((meta.max - 1.0).abs() < 1e-6);
        assert!((meta.rms - 0.75).abs() < 1e-6);

        let loaded = load(&path).expect("load");
        let mut samples = [0.0_f32; 4];
        assert_eq!(loaded.read(0, 4, &mut samples), 4);
        assert_eq!(samples, [0.0, 1.0, -1.0, 0.5]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn version_1_sidecar_still_loads() {
        let path = temp_path("v1");
        fs::write(&path, version_1_bytes(44_100, &[0.25, -0.5, 0.75])).expect("write v1");

        let meta = load_meta(&path).expect_err("version 1 has no metadata");
        assert_eq!(meta.kind(), io::ErrorKind::Unsupported);

        let loaded = load(&path).expect("load v1");
        assert_eq!(loaded.sample_rate(), 44_100);
        let mut samples = [0.0_f32; 3];
        assert_eq!(loaded.read(0, 3, &mut samples), 3);
        assert_eq!(samples, [0.25, -0.5, 0.75]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn metadata_and_load_reject_a_frame_count_mismatch() {
        let path = temp_path("mismatch");
        let sequence = Sequence::from_samples(48_000, vec![0.0, 1.0]).expect("sequence");
        save(&sequence, &path).expect("save");

        let mut bytes = fs::read(&path).expect("read");
        bytes[24..32].copy_from_slice(&7_u64.to_le_bytes());
        fs::write(&path, &bytes).expect("rewrite");

        assert!(load_meta(&path).is_err());
        assert!(load(&path).is_err());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_short_v2_file_is_rejected() {
        let path = temp_path("short");
        let sequence = Sequence::from_samples(44_100, vec![0.0]).expect("sequence");
        save(&sequence, &path).expect("save");

        let bytes = fs::read(&path).expect("read");
        fs::write(&path, &bytes[..40]).expect("truncate");
        assert!(load_meta(&path).is_err());
        assert!(load(&path).is_err());

        let _ = fs::remove_file(path);
    }
}
