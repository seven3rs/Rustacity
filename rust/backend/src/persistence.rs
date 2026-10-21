// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The on-disk sidecar for a Rust-backed sequence.
//!
//! A `.rsq` file is deliberately small and versioned: a fixed 32-byte header
//! followed by little-endian `f32` samples.  The C++ `RustBlockFile` owns the
//! file name and the `DirManager` lifecycle; this module only writes and reads
//! the bytes so the header stays in one place.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::Sequence;

const MAGIC: &[u8; 8] = b"RUSTSEQ1";
const VERSION: u32 = 1;
const CHANNELS: u32 = 1;
const FLOAT_FORMAT: u32 = 1;
const HEADER_LEN: usize = 32;
const COPY_CHUNK: usize = 64 * 1024;
const TEMP_SUFFIX: &str = ".tmp";

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(TEMP_SUFFIX);
    PathBuf::from(name)
}

/// Write `sequence` to `path` with a header that identifies the format.
///
/// The data is written to a sibling temporary file first, so a failed save
/// cannot truncate a sidecar that an already-saved project still references.
/// The temporary file is renamed into place only after every sample is flushed.
pub fn save(sequence: &Sequence, path: &Path) -> io::Result<()> {
    let frames = sequence.frames();
    let sample_rate = sequence.sample_rate();
    if sample_rate == 0 {
        return Err(invalid("a sequence needs a sample rate"));
    }

    let temporary = temp_path(path);
    let result = write_sequence(sequence, &temporary, frames, sample_rate);
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

fn write_sequence(
    sequence: &Sequence,
    path: &Path,
    frames: u64,
    sample_rate: u32,
) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);

    let mut header = [0_u8; HEADER_LEN];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&sample_rate.to_le_bytes());
    header[16..20].copy_from_slice(&CHANNELS.to_le_bytes());
    header[20..24].copy_from_slice(&FLOAT_FORMAT.to_le_bytes());
    header[24..32].copy_from_slice(&frames.to_le_bytes());
    writer.write_all(&header)?;

    let mut samples = vec![0.0_f32; COPY_CHUNK];
    let mut bytes = Vec::with_capacity(COPY_CHUNK * 4);
    let mut position = 0_u64;
    while position < frames {
        let count = usize::try_from((frames - position).min(COPY_CHUNK as u64))
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
    if file_len < HEADER_LEN as u64 {
        return Err(invalid("the sequence file is shorter than its header"));
    }

    let mut file = File::open(path)?;
    let mut header = [0_u8; HEADER_LEN];
    file.read_exact(&mut header)?;

    if &header[..8] != MAGIC {
        return Err(invalid("the sequence file has an unknown signature"));
    }
    let version = u32::from_le_bytes(header[8..12].try_into().expect("four-byte field"));
    if version != VERSION {
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
    let data_len = file_len - HEADER_LEN as u64;
    if frames.checked_mul(4) != Some(data_len) {
        return Err(invalid(
            "the sequence file length does not match its frame count",
        ));
    }
    let frames = usize::try_from(frames)
        .map_err(|_| invalid("the sequence is too long for this platform"))?;
    let byte_len = frames
        .checked_mul(4)
        .ok_or_else(|| invalid("the sequence is too long for this platform"))?;

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
