// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Backend tests: edit operations, WAV channel splitting and the C ABI.

use std::ffi::CString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rustacity_backend::ffi::{
    rustacity_import_channel_count, rustacity_import_free, rustacity_import_sample_rate,
    rustacity_import_take_sequence, rustacity_sequence_append_f32, rustacity_sequence_clone,
    rustacity_sequence_delete, rustacity_sequence_frames, rustacity_sequence_free,
    rustacity_sequence_insert_f32, rustacity_sequence_insert_silence, rustacity_sequence_load,
    rustacity_sequence_load_meta, rustacity_sequence_min_max, rustacity_sequence_new_silence,
    rustacity_sequence_read_f32, rustacity_sequence_rms, rustacity_sequence_sample_rate,
    rustacity_sequence_save, rustacity_sequence_set_silence, rustacity_sequence_slice,
    rustacity_sequence_wave_display, rustacity_sequence_write_f32, rustacity_wav_import_open,
};
use rustacity_backend::Sequence;

fn temp_path(label: &str, extension: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "rustacity-backend-{}-{stamp}-{label}.{extension}",
        std::process::id()
    ))
}

fn temp_wav_path(label: &str) -> PathBuf {
    temp_path(label, "wav")
}

fn write_stereo_wav(path: &PathBuf) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create WAV");
    writer.write_sample(16_384i16).expect("left");
    writer.write_sample(-16_384i16).expect("right");
    writer.write_sample(0i16).expect("left");
    writer.write_sample(0i16).expect("right");
    writer.finalize().expect("finish WAV");
}

#[test]
fn edit_operations_keep_the_expected_lengths_and_values() {
    let sequence = Sequence::from_samples(8_000, vec![0.0, 1.0, -1.0, 0.5]).expect("sequence");

    assert_eq!(sequence.frames(), 4);
    assert_eq!(sequence.min_max(1, 2), (-1.0, 1.0));
    assert!((sequence.rms(0, 4) - 0.75).abs() < 1e-6);

    let mut samples = [0.0; 4];
    assert_eq!(sequence.read(0, 4, &mut samples), 4);
    assert_eq!(samples, [0.0, 1.0, -1.0, 0.5]);

    sequence.delete(1, 1);
    assert_eq!(sequence.frames(), 3);
    let mut samples = [0.0; 3];
    sequence.read(0, 3, &mut samples);
    assert_eq!(samples, [0.0, -1.0, 0.5]);

    sequence.insert_silence(1, 2);
    assert_eq!(sequence.frames(), 5);
    let mut samples = [0.0; 5];
    sequence.read(0, 5, &mut samples);
    assert_eq!(samples, [0.0, 0.0, 0.0, -1.0, 0.5]);

    sequence.set_silence(0, 2);
    let mut samples = [0.0; 5];
    sequence.read(0, 5, &mut samples);
    assert_eq!(samples, [0.0, 0.0, 0.0, -1.0, 0.5]);

    sequence.write(0, &[0.25]);
    sequence.append(&[0.75]);
    assert_eq!(sequence.frames(), 6);
    let mut samples = [0.0; 6];
    sequence.read(0, 6, &mut samples);
    assert_eq!(samples, [0.25, 0.0, 0.0, -1.0, 0.5, 0.75]);

    sequence.insert(2, &[0.125, -0.125]);
    assert_eq!(sequence.frames(), 8);
    let mut samples = [0.0; 8];
    sequence.read(0, 8, &mut samples);
    assert_eq!(samples, [0.25, 0.0, 0.125, -0.125, 0.0, -1.0, 0.5, 0.75]);

    let copy = sequence.clone();
    copy.write(0, &[9.0]);
    let mut original = [0.0; 1];
    let mut copied = [0.0; 1];
    sequence.read(0, 1, &mut original);
    copy.read(0, 1, &mut copied);
    assert_eq!(original, [0.25]);
    assert_eq!(copied, [9.0]);
}

#[test]
fn wave_display_returns_one_column_per_pixel() {
    let sequence = Sequence::from_samples(8_000, vec![0.0, 1.0, -1.0, 0.5]).expect("sequence");
    let mut where_out = [0_u64; 3];
    let mut min = [0.0_f32; 2];
    let mut max = [0.0_f32; 2];
    let mut rms = [0.0_f32; 2];
    let mut bl = [0_i32; 2];

    sequence.wave_display(
        0.0,
        4_000.0,
        2,
        &mut where_out,
        &mut min,
        &mut max,
        &mut rms,
        &mut bl,
    );

    assert_eq!(where_out, [0, 2, 4]);
    assert_eq!(min, [0.0, -1.0]);
    assert_eq!(max, [1.0, 0.5]);
    assert_eq!(bl, [1, 1]);
    assert!(rms[0] > 0.0);
    assert!(rms[1] > 0.0);
}

#[test]
fn wav_import_splits_channels_and_the_c_abi_reads_and_edits_them() {
    let path = temp_wav_path("stereo16");
    write_stereo_wav(&path);

    let c_path = CString::new(path.to_str().expect("UTF-8 temp path")).expect("no NUL");
    let import = unsafe { rustacity_wav_import_open(c_path.as_ptr()) };
    assert!(!import.is_null());

    let channels = unsafe { rustacity_import_channel_count(import) };
    let sample_rate = unsafe { rustacity_import_sample_rate(import) };
    assert_eq!(channels, 2);
    assert_eq!(sample_rate, 8_000);

    let left = unsafe { rustacity_import_take_sequence(import, 0) };
    let right = unsafe { rustacity_import_take_sequence(import, 1) };
    assert!(!left.is_null());
    assert!(!right.is_null());

    assert_eq!(unsafe { rustacity_sequence_frames(left) }, 2);

    let mut samples = [0.0_f32; 2];
    let read = unsafe { rustacity_sequence_read_f32(left, 0, 2, samples.as_mut_ptr()) };
    assert_eq!(read, 2);
    assert!((samples[0] - 0.5).abs() < 1e-6);
    assert!(samples[1].abs() < 1e-6);

    let mut min = 0.0;
    let mut max = 0.0;
    assert_eq!(
        unsafe { rustacity_sequence_min_max(left, 0, 2, &mut min, &mut max) },
        1
    );
    assert!((min - 0.0).abs() < 1e-6);
    assert!((max - 0.5).abs() < 1e-6);

    let mut where_out = [0_u64; 3];
    let mut col_min = [0.0_f32; 2];
    let mut col_max = [0.0_f32; 2];
    let mut rms = [0.0_f32; 2];
    let mut bl = [0_i32; 2];
    let displayed = unsafe {
        rustacity_sequence_wave_display(
            left,
            0.0,
            4_000.0,
            2,
            where_out.as_mut_ptr(),
            col_min.as_mut_ptr(),
            col_max.as_mut_ptr(),
            rms.as_mut_ptr(),
            bl.as_mut_ptr(),
        )
    };
    assert_eq!(displayed, 1);
    assert_eq!(where_out, [0, 2, 2]);
    assert_eq!(col_min, [0.0, 0.0]);
    assert_eq!(col_max, [0.5, 0.0]);

    unsafe {
        assert_eq!(rustacity_sequence_append_f32(left, 1, [0.25].as_ptr()), 1);
        assert_eq!(
            rustacity_sequence_insert_f32(left, 1, 2, [0.1, -0.1].as_ptr()),
            1
        );
        assert_eq!(rustacity_sequence_delete(left, 0, 1), 1);
        assert_eq!(rustacity_sequence_set_silence(left, 0, 1), 1);
        assert_eq!(rustacity_sequence_insert_silence(left, 4, 1), 1);
        assert_eq!(rustacity_sequence_write_f32(left, 0, 1, [0.75].as_ptr()), 1);
    }
    assert_eq!(unsafe { rustacity_sequence_frames(left) }, 5);
    assert!(unsafe { rustacity_sequence_rms(left, 0, 5) } > 0.0);

    let mut edited = [0.0_f32; 5];
    let read = unsafe { rustacity_sequence_read_f32(left, 0, 5, edited.as_mut_ptr()) };
    assert_eq!(read, 5);
    assert_eq!(edited, [0.75, -0.1, 0.0, 0.25, 0.0]);

    let copy = unsafe { rustacity_sequence_clone(left) };
    assert!(!copy.is_null());
    unsafe {
        assert_eq!(
            rustacity_sequence_write_f32(copy, 0, 1, [-0.75].as_ptr()),
            1
        );
    }
    let mut copied = [0.0_f32; 1];
    unsafe {
        rustacity_sequence_read_f32(copy, 0, 1, copied.as_mut_ptr());
    }
    assert_eq!(copied, [-0.75]);
    unsafe {
        rustacity_sequence_read_f32(left, 0, 1, edited.as_mut_ptr());
    }
    assert_eq!(edited[0], 0.75);

    let slice = unsafe { rustacity_sequence_slice(left, 1, 3) };
    assert!(!slice.is_null());
    assert_eq!(unsafe { rustacity_sequence_frames(slice) }, 3);
    let mut sliced = [0.0_f32; 3];
    unsafe {
        rustacity_sequence_read_f32(slice, 0, 3, sliced.as_mut_ptr());
        rustacity_sequence_free(copy);
        rustacity_sequence_free(slice);
    }
    assert_eq!(sliced, [-0.1, 0.0, 0.25]);

    let mut right_samples = [0.0_f32; 2];
    let read = unsafe { rustacity_sequence_read_f32(right, 0, 2, right_samples.as_mut_ptr()) };
    assert_eq!(read, 2);
    assert!((right_samples[0] + 0.5).abs() < 1e-6);

    unsafe {
        rustacity_sequence_free(left);
        rustacity_sequence_free(right);
        rustacity_import_free(import);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn sequence_sidecar_round_trips_samples_and_rate() {
    let path = temp_path("sidecar", "rsq");
    let c_path = CString::new(path.to_str().expect("UTF-8 temp path")).expect("no NUL");

    let source = unsafe { rustacity_sequence_new_silence(0, 48_000) };
    assert!(!source.is_null());
    assert_eq!(unsafe { rustacity_sequence_sample_rate(source) }, 48_000);
    let samples = [0.125_f32, -0.25, 0.5, 0.0];
    unsafe {
        assert_eq!(
            rustacity_sequence_write_f32(source, 0, samples.len(), samples.as_ptr()),
            1
        );
        assert_eq!(rustacity_sequence_save(source, c_path.as_ptr()), 1);
    }

    let loaded = unsafe { rustacity_sequence_load(c_path.as_ptr()) };
    assert!(!loaded.is_null());
    assert_eq!(unsafe { rustacity_sequence_frames(loaded) }, 4);
    assert_eq!(unsafe { rustacity_sequence_sample_rate(loaded) }, 48_000);
    let mut restored = [0.0_f32; 4];
    assert_eq!(
        unsafe { rustacity_sequence_read_f32(loaded, 0, 4, restored.as_mut_ptr()) },
        4
    );
    assert_eq!(restored, samples);

    unsafe {
        rustacity_sequence_free(source);
        rustacity_sequence_free(loaded);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn sequence_sidecar_replaces_an_existing_file() {
    let path = temp_path("replaced-sidecar", "rsq");
    let c_path = CString::new(path.to_str().expect("UTF-8 temp path")).expect("no NUL");

    let source = unsafe { rustacity_sequence_new_silence(0, 44_100) };
    assert!(!source.is_null());
    let samples = [0.5_f32, -0.5];
    unsafe {
        assert_eq!(
            rustacity_sequence_write_f32(source, 0, samples.len(), samples.as_ptr()),
            1
        );
        assert_eq!(rustacity_sequence_save(source, c_path.as_ptr()), 1);
        assert_eq!(rustacity_sequence_set_silence(source, 0, 2), 1);
        assert_eq!(rustacity_sequence_save(source, c_path.as_ptr()), 1);
    }

    let loaded = unsafe { rustacity_sequence_load(c_path.as_ptr()) };
    assert!(!loaded.is_null());
    assert_eq!(unsafe { rustacity_sequence_frames(loaded) }, 2);
    let mut restored = [1.0_f32; 2];
    assert_eq!(
        unsafe { rustacity_sequence_read_f32(loaded, 0, 2, restored.as_mut_ptr()) },
        2
    );
    assert_eq!(restored, [0.0, 0.0]);

    unsafe {
        rustacity_sequence_free(source);
        rustacity_sequence_free(loaded);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn sequence_sidecar_round_trips_an_empty_sequence() {
    let path = temp_path("empty-sidecar", "rsq");
    let c_path = CString::new(path.to_str().expect("UTF-8 temp path")).expect("no NUL");

    let source = unsafe { rustacity_sequence_new_silence(0, 44_100) };
    assert!(!source.is_null());
    assert_eq!(
        unsafe { rustacity_sequence_save(source, c_path.as_ptr()) },
        1
    );

    let loaded = unsafe { rustacity_sequence_load(c_path.as_ptr()) };
    assert!(!loaded.is_null());
    assert_eq!(unsafe { rustacity_sequence_frames(loaded) }, 0);
    assert_eq!(unsafe { rustacity_sequence_sample_rate(loaded) }, 44_100);

    unsafe {
        rustacity_sequence_free(source);
        rustacity_sequence_free(loaded);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn sequence_sidecar_rejects_broken_files() {
    let path = temp_path("broken", "rsq");
    std::fs::write(&path, b"not a sequence").expect("write broken sidecar");
    let c_path = CString::new(path.to_str().expect("UTF-8 temp path")).expect("no NUL");
    let loaded = unsafe { rustacity_sequence_load(c_path.as_ptr()) };
    assert!(loaded.is_null());
    let missing = unsafe { rustacity_sequence_load(c_path.as_ptr()) };
    assert!(missing.is_null());
    let _ = std::fs::remove_file(path);
}

/// A version-1 sidecar: the 32-byte header the backend wrote before the v2
/// metadata section existed.
fn write_version_1_sidecar(path: &PathBuf, sample_rate: u32, samples: &[f32]) {
    let mut bytes = vec![0_u8; 32];
    bytes[..8].copy_from_slice(b"RUSTSEQ1");
    bytes[8..12].copy_from_slice(&1_u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&sample_rate.to_le_bytes());
    bytes[16..20].copy_from_slice(&1_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&(samples.len() as u64).to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write v1 sidecar");
}

#[test]
fn sequence_metadata_reads_without_loading_samples() {
    let path = temp_path("metadata", "rsq");
    let c_path = CString::new(path.to_str().expect("UTF-8 temp path")).expect("no NUL");

    let source = unsafe { rustacity_sequence_new_silence(0, 8_000) };
    assert!(!source.is_null());
    let samples = [0.0_f32, 1.0, -1.0, 0.5];
    unsafe {
        assert_eq!(
            rustacity_sequence_write_f32(source, 0, samples.len(), samples.as_ptr()),
            1
        );
        assert_eq!(rustacity_sequence_save(source, c_path.as_ptr()), 1);
    }

    let mut frames = 0_u64;
    let mut rate = 0_u32;
    let mut min = 0.0_f32;
    let mut max = 0.0_f32;
    let mut rms = 0.0_f32;
    assert_eq!(
        unsafe {
            rustacity_sequence_load_meta(
                c_path.as_ptr(),
                &mut frames,
                &mut rate,
                &mut min,
                &mut max,
                &mut rms,
            )
        },
        1
    );
    assert_eq!(frames, 4);
    assert_eq!(rate, 8_000);
    assert!((min + 1.0).abs() < 1e-6);
    assert!((max - 1.0).abs() < 1e-6);
    assert!((rms - 0.75).abs() < 1e-6);

    unsafe { rustacity_sequence_free(source) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn sequence_metadata_reports_version_1_as_missing() {
    let path = temp_path("v1-metadata", "rsq");
    write_version_1_sidecar(&path, 44_100, &[0.25, -0.5, 0.75]);
    let c_path = CString::new(path.to_str().expect("UTF-8 temp path")).expect("no NUL");

    let mut frames = 0_u64;
    let mut rate = 0_u32;
    let mut min = 0.0_f32;
    let mut max = 0.0_f32;
    let mut rms = 0.0_f32;
    assert_eq!(
        unsafe {
            rustacity_sequence_load_meta(
                c_path.as_ptr(),
                &mut frames,
                &mut rate,
                &mut min,
                &mut max,
                &mut rms,
            )
        },
        0
    );

    // The version-1 reader still exposes the clip through the full load path,
    // so the C++ fallback keeps working.
    let loaded = unsafe { rustacity_sequence_load(c_path.as_ptr()) };
    assert!(!loaded.is_null());
    assert_eq!(unsafe { rustacity_sequence_frames(loaded) }, 3);
    assert_eq!(unsafe { rustacity_sequence_sample_rate(loaded) }, 44_100);
    let mut restored = [0.0_f32; 3];
    assert_eq!(
        unsafe { rustacity_sequence_read_f32(loaded, 0, 3, restored.as_mut_ptr()) },
        3
    );
    assert_eq!(restored, [0.25, -0.5, 0.75]);

    // A null path is rejected by the validated entry point.
    assert_eq!(
        unsafe {
            rustacity_sequence_load_meta(
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );

    unsafe {
        rustacity_sequence_free(loaded);
    }
    let _ = std::fs::remove_file(path);
}
