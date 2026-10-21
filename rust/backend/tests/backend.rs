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
    rustacity_sequence_insert_f32, rustacity_sequence_insert_silence, rustacity_sequence_min_max,
    rustacity_sequence_read_f32, rustacity_sequence_rms, rustacity_sequence_set_silence,
    rustacity_sequence_slice, rustacity_sequence_wave_display, rustacity_sequence_write_f32,
    rustacity_wav_import_open,
};
use rustacity_backend::Sequence;

fn temp_wav_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "rustacity-backend-{}-{stamp}-{label}.wav",
        std::process::id()
    ))
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
