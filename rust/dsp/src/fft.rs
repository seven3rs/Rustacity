// Rustacity: A Digital Audio Editor
//
// Copyright (C) 2026 Sato
//
// SPDX-License-Identifier: GPL-2.0-or-later

//! The real and complex FFTs and the window functions.
//!
//! Port of the arithmetic that used to live in Philip Van Baren's
//! `src/RealFFTf.cpp` and Dominic Mazzoni's `src/FFT.cpp`, extracted into
//! `src/FFTMath.cpp` for capture.  The C++ side keeps what is not
//! arithmetic: the pool of tables and the lock that guards it, the scratch
//! buffers of its callers, the argument checks and the translated window
//! names.
//!
//! The arithmetic is transcribed operation for operation, because the golden
//! values in `rust/dsp/goldens/reference.txt` were captured from the C++ and
//! pin every result, rounding included.  That is why some expressions below
//! deliberately work in `f64` and some deliberately do not: it follows the
//! types of the C++, where a window function that multiplies by a `double`
//! coefficient rounds once, in `f64`, while one that multiplies by a `float`
//! rounds in `f32`.
//!
//! The tables are plain slices here, exactly as the parameters of
//! `src/FFTMath.cpp`: `bit_reversed` holds `fftlen / 2` ints and `sin_table`
//! holds `fftlen` floats, and the caller owns both.  That is the shape the C
//! ABI in `rust/dsp/include/rustacity_dsp.h` passes them in.
//!
//! Contributor: Sato <seven3rs@gmail.com>

use std::sync::{Arc, Mutex};

use std::f64::consts::PI;

/// The window functions of `enum eWindowFunctions`, in the order the C++
/// enumeration declares them.  The names are translated on the C++ side.
pub const WINDOW_RECTANGULAR: i32 = 0;
pub const WINDOW_BARTLETT: i32 = 1;
pub const WINDOW_HAMMING: i32 = 2;
pub const WINDOW_HANNING: i32 = 3;
pub const WINDOW_BLACKMAN: i32 = 4;
pub const WINDOW_BLACKMAN_HARRIS: i32 = 5;
pub const WINDOW_WELCH: i32 = 6;
pub const WINDOW_GAUSSIAN25: i32 = 7;
pub const WINDOW_GAUSSIAN35: i32 = 8;
pub const WINDOW_GAUSSIAN45: i32 = 9;

/// `eWinFuncCount`: how many window functions there are, which is also the
/// value that is one past the last selector.
pub const WINDOW_COUNT: i32 = 10;

/// `InitializeFFT` of the C++ tree: the bit-reversal pointers and the sine
/// table for a transform of `fftlen` samples.
///
/// `bit_reversed` must hold `fftlen / 2` ints and `sin_table` `fftlen` floats.
pub fn init_tables(fftlen: usize, bit_reversed: &mut [i32], sin_table: &mut [f32]) {
    let points = fftlen / 2;
    debug_assert_eq!(bit_reversed.len(), points);
    debug_assert_eq!(sin_table.len(), 2 * points);

    for i in 0..points {
        let mut temp = 0i32;
        let mut mask = points / 2;
        while mask > 0 {
            temp = (temp >> 1) + if i & mask != 0 { points as i32 } else { 0 };
            mask >>= 1;
        }
        bit_reversed[i] = temp;
    }

    for i in 0..points {
        let angle = 2.0 * PI * i as f64 / (2 * points) as f64;
        let at = bit_reversed[i] as usize;
        sin_table[at] = -(angle.sin()) as f32;
        sin_table[at + 1] = -(angle.cos()) as f32;
    }
}

/// The forward transform of `RealFFTf`, in place.
///
/// `buffer` holds the whole transform: two floats per complex bin.  The output
/// is bit-reversed, and the Fs/2 component is left in the imaginary half of
/// the DC bin, so use [`reorder_to_freq`] to read it.
pub fn forward(buffer: &mut [f32], bit_reversed: &[i32], sin_table: &[f32]) {
    let fftlen = buffer.len();
    let points = fftlen / 2;
    debug_assert!(fftlen >= 4);
    debug_assert_eq!(bit_reversed.len(), points);
    debug_assert_eq!(sin_table.len(), fftlen);

    let mut butterflies_per_group = points / 2;
    while butterflies_per_group > 0 {
        let mut a = 0usize;
        let mut b = butterflies_per_group * 2;
        let mut s = 0usize;

        while a < fftlen {
            let sin = sin_table[s];
            let cos = sin_table[s + 1];
            let end = b;

            while a < end {
                let v1 = buffer[b] * cos + buffer[b + 1] * sin;
                let v2 = buffer[b] * sin - buffer[b + 1] * cos;
                buffer[b] = buffer[a] + v1;
                buffer[a] = buffer[b] - 2.0 * v1;
                a += 1;
                b += 1;
                buffer[b] = buffer[a] - v2;
                buffer[a] = buffer[b] + 2.0 * v2;
                a += 1;
                b += 1;
            }

            a = b;
            b += butterflies_per_group * 2;
            s += 2;
        }

        butterflies_per_group >>= 1;
    }

    // Massage the output to get the transform of a real input sequence.
    let mut br1 = 1usize;
    let mut br2 = points - 1;
    while br1 < br2 {
        let at1 = bit_reversed[br1] as usize;
        let at2 = bit_reversed[br2] as usize;
        let sin = sin_table[at1];
        let cos = sin_table[at1 + 1];

        let hrminus = buffer[at1] - buffer[at2];
        let hrplus = hrminus + buffer[at2] * 2.0;
        let himinus = buffer[at1 + 1] - buffer[at2 + 1];
        let hiplus = himinus + buffer[at2 + 1] * 2.0;

        let v1 = sin * hrminus - cos * hiplus;
        let v2 = cos * hrminus + sin * hiplus;

        buffer[at1] = (hrplus + v1) * 0.5;
        buffer[at2] = buffer[at1] - v1;
        buffer[at1 + 1] = (himinus + v2) * 0.5;
        buffer[at2 + 1] = buffer[at1 + 1] - himinus;

        br1 += 1;
        br2 -= 1;
    }

    // The center bin needs a conjugate only.
    let at = bit_reversed[br1] as usize + 1;
    buffer[at] = -buffer[at];

    // Put the Fs/2 value into the imaginary part of the DC bin.
    let v1 = buffer[0] - buffer[1];
    buffer[0] += buffer[1];
    buffer[1] = v1;
}

/// The inverse transform of `InverseRealFFTf`, in place, of bit-reversed
/// conjugate-symmetric input.
///
/// The input is interleaved (real, imaginary) complex data with the Fs/2
/// component in the imaginary half of the DC bin, which is what
/// [`inverse_real_fft`] builds.
pub fn inverse(buffer: &mut [f32], bit_reversed: &[i32], sin_table: &[f32]) {
    let fftlen = buffer.len();
    let points = fftlen / 2;
    debug_assert!(fftlen >= 4);
    debug_assert_eq!(bit_reversed.len(), points);
    debug_assert_eq!(sin_table.len(), fftlen);

    let mut butterflies_per_group = points / 2;

    // Massage the input to get the input for a real output sequence.
    let mut a = 2usize;
    let mut b = points * 2 - 2;
    let mut br = 1usize;
    while a < b {
        let at1 = bit_reversed[br] as usize;
        let sin = sin_table[at1];
        let cos = sin_table[at1 + 1];

        let hrminus = buffer[a] - buffer[b];
        let hrplus = hrminus + buffer[b] * 2.0;
        let himinus = buffer[a + 1] - buffer[b + 1];
        let hiplus = himinus + buffer[b + 1] * 2.0;

        let v1 = sin * hrminus + cos * hiplus;
        let v2 = cos * hrminus - sin * hiplus;

        buffer[a] = (hrplus + v1) * 0.5;
        buffer[b] = buffer[a] - v1;
        buffer[a + 1] = (himinus - v2) * 0.5;
        buffer[b + 1] = buffer[a + 1] - himinus;

        a += 2;
        b -= 2;
        br += 1;
    }

    // The center bin needs a conjugate only.
    buffer[a + 1] = -buffer[a + 1];

    // The DC bin is passed in as the real part of the DC complex value and the
    // Fs/2 bin as the imaginary part: (v1 + v2) is the DC component and
    // (v1 - v2) the Fs/2 one.
    let v1 = 0.5 * (buffer[0] + buffer[1]);
    let v2 = 0.5 * (buffer[0] - buffer[1]);
    buffer[0] = v1;
    buffer[1] = v2;

    while butterflies_per_group > 0 {
        let mut a = 0usize;
        let mut b = butterflies_per_group * 2;
        let mut s = 0usize;

        while a < fftlen {
            let sin = sin_table[s];
            s += 1;
            let cos = sin_table[s];
            s += 1;
            let end = b;

            while a < end {
                let v1 = buffer[b] * cos - buffer[b + 1] * sin;
                let v2 = buffer[b] * sin + buffer[b + 1] * cos;
                buffer[b] = (buffer[a] + v1) * 0.5;
                buffer[a] = buffer[b] - v1;
                a += 1;
                b += 1;
                buffer[b] = (buffer[a] + v2) * 0.5;
                buffer[a] = buffer[b] - v2;
                a += 1;
                b += 1;
            }

            a = b;
            b += butterflies_per_group * 2;
        }

        butterflies_per_group >>= 1;
    }
}

/// Undo the bit-reversal of a forward transform, giving the bins in order.
///
/// `real_out` and `imag_out` hold one value per bin, DC and Fs/2 included,
/// that is `fftlen / 2 + 1` values.
pub fn reorder_to_freq(
    bit_reversed: &[i32],
    buffer: &[f32],
    real_out: &mut [f32],
    imag_out: &mut [f32],
) {
    let points = buffer.len() / 2;
    debug_assert_eq!(bit_reversed.len(), points);
    debug_assert!(real_out.len() >= points + 1);
    debug_assert!(imag_out.len() >= points + 1);

    for i in 1..points {
        let at = bit_reversed[i] as usize;
        real_out[i] = buffer[at];
        imag_out[i] = buffer[at + 1];
    }

    // The DC component, and the Fs/2 one, which are both real only.
    real_out[0] = buffer[0];
    imag_out[0] = 0.0;
    real_out[points] = buffer[1];
    imag_out[points] = 0.0;
}

/// Undo the bit-reversal of an inverse transform, giving the real samples
/// interleaved two to a bin.
pub fn reorder_to_time(bit_reversed: &[i32], buffer: &[f32], time_out: &mut [f32]) {
    let points = buffer.len() / 2;
    debug_assert_eq!(bit_reversed.len(), points);
    debug_assert!(time_out.len() >= 2 * points);

    for i in 0..points {
        let at = bit_reversed[i] as usize;
        time_out[i * 2] = buffer[at];
        time_out[i * 2 + 1] = buffer[at + 1];
    }
}

/// `RealFFT` of the C++ tree: the transform of a real signal, from which the
/// conjugate-symmetric spectrum follows.
///
/// `real_out` and `imag_out` hold `real_in.len()` values each, the upper half
/// filled in from the symmetry.
pub fn real_fft(
    real_in: &[f32],
    real_out: &mut [f32],
    imag_out: &mut [f32],
    bit_reversed: &[i32],
    sin_table: &[f32],
) {
    let num_samples = real_in.len();
    let points = num_samples / 2;
    debug_assert_eq!(real_out.len(), num_samples);
    debug_assert_eq!(imag_out.len(), num_samples);

    let mut buffer = real_in.to_vec();
    forward(&mut buffer, bit_reversed, sin_table);

    for i in 1..points {
        let at = bit_reversed[i] as usize;
        real_out[i] = buffer[at];
        imag_out[i] = buffer[at + 1];
    }

    // The (real only) DC and Fs/2 bins.
    real_out[0] = buffer[0];
    real_out[points] = buffer[1];
    imag_out[0] = 0.0;
    imag_out[points] = 0.0;

    for i in (points + 1)..num_samples {
        real_out[i] = real_out[num_samples - i];
        imag_out[i] = -imag_out[num_samples - i];
    }
}

/// `InverseRealFFT` of the C++ tree: the inverse of [`real_fft`], whose input
/// is assumed conjugate-symmetric, so only the first half of it is read.
///
/// `imag_in` may be absent, in which case the imaginary parts are taken as
/// zero.
pub fn inverse_real_fft(
    real_in: &[f32],
    imag_in: Option<&[f32]>,
    real_out: &mut [f32],
    bit_reversed: &[i32],
    sin_table: &[f32],
) {
    let num_samples = real_in.len();
    let points = num_samples / 2;
    debug_assert_eq!(real_out.len(), num_samples);

    let mut buffer = vec![0.0f32; num_samples];
    for i in 0..points {
        buffer[2 * i] = real_in[i];
        buffer[2 * i + 1] = imag_in.map_or(0.0, |imag| imag[i]);
    }
    // Put the Fs/2 component in the imaginary part of the DC bin.
    buffer[1] = real_in[points];

    inverse(&mut buffer, bit_reversed, sin_table);
    reorder_to_time(bit_reversed, &buffer, real_out);
}

/// `PowerSpectrum` of the C++ tree: the squared magnitude of each coefficient
/// of the real transform, which is the power and nothing of the phase.
///
/// `output` holds `input.len() / 2 + 1` values.
pub fn power_spectrum(
    input: &[f32],
    output: &mut [f32],
    bit_reversed: &[i32],
    sin_table: &[f32],
) {
    let num_samples = input.len();
    debug_assert!(output.len() >= num_samples / 2 + 1);

    let mut buffer = input.to_vec();
    forward(&mut buffer, bit_reversed, sin_table);

    for i in 1..(num_samples / 2) {
        let at = bit_reversed[i] as usize;
        output[i] = buffer[at] * buffer[at] + buffer[at + 1] * buffer[at + 1];
    }

    // The (real only) DC and Fs/2 bins.
    output[0] = buffer[0] * buffer[0];
    output[num_samples / 2] = buffer[1] * buffer[1];
}

/// How many bits it takes to hold the indices of a transform of
/// `power_of_two` samples - `NumberOfBitsNeeded` in the C++.
fn number_of_bits_needed(power_of_two: usize) -> u32 {
    let mut bits = 0;
    let mut remaining = power_of_two;
    while remaining > 1 {
        remaining >>= 1;
        bits += 1;
    }
    bits
}

/// `ReverseBits` of the C++ tree.
fn reverse_bits(index: usize, num_bits: u32) -> usize {
    let mut rev = 0usize;
    let mut index = index;
    for _ in 0..num_bits {
        rev = (rev << 1) | (index & 1);
        index >>= 1;
    }
    rev
}

/// The largest transform the bit table below is built for, `MaxFastBits` in
/// the C++: up to 2^16 samples are reversed by table lookup.
const MAX_FAST_BITS: u32 = 16;

/// Build the bit-reversal table of the complex transform, which used to be
/// `gFFTBitTable` and `InitFFT` in the C++.
fn build_bit_table() -> Vec<Vec<i32>> {
    let mut table = Vec::with_capacity(MAX_FAST_BITS as usize);
    let mut len = 2usize;
    for bits in 1..=MAX_FAST_BITS {
        let mut array = Vec::with_capacity(len);
        for i in 0..len {
            array.push(reverse_bits(i, bits) as i32);
        }
        table.push(array);
        len <<= 1;
    }
    table
}

/// The bit table of the complex transform, built on first use and dropped by
/// [`deinit_fft`], as the C++ built and freed `gFFTBitTable`.
static BIT_TABLE: Mutex<Option<Arc<Vec<Vec<i32>>>>> = Mutex::new(None);

/// The table, building it if this is the first call.  The `Arc` is cloned out
/// from under the lock so that a transform does not hold it.
fn bit_table() -> Arc<Vec<Vec<i32>>> {
    let mut table = BIT_TABLE.lock().unwrap_or_else(|e| e.into_inner());
    if table.is_none() {
        *table = Some(Arc::new(build_bit_table()));
    }
    Arc::clone(table.as_ref().unwrap())
}

/// `FastReverseBits` of the C++ tree.
fn fast_reverse_bits(table: &[Vec<i32>], i: usize, num_bits: u32) -> usize {
    if num_bits <= MAX_FAST_BITS {
        table[num_bits as usize - 1][i] as usize
    } else {
        reverse_bits(i, num_bits)
    }
}

/// `DeinitFFT` of the C++ tree: release the bit table of the complex
/// transform.
pub fn deinit_fft() {
    let mut table = BIT_TABLE.lock().unwrap_or_else(|e| e.into_inner());
    *table = None;
}

/// The complex transform of `FFT()` in the C++ tree.
///
/// The four buffers hold `real_in.len()` values each, which must be a power of
/// two and at least two.  A forward transform is the plain sum of the samples
/// against the roots of unity and the inverse divides by the length, so the
/// two are exact inverses of each other.
pub fn fft(
    real_in: &[f32],
    imag_in: Option<&[f32]>,
    real_out: &mut [f32],
    imag_out: &mut [f32],
    inverse_transform: bool,
) {
    let num_samples = real_in.len();
    debug_assert!(num_samples >= 2 && num_samples.is_power_of_two());
    debug_assert_eq!(real_out.len(), num_samples);
    debug_assert_eq!(imag_out.len(), num_samples);

    let mut angle_numerator = 2.0 * PI;
    if !inverse_transform {
        angle_numerator = -angle_numerator;
    }

    let num_bits = number_of_bits_needed(num_samples);
    let table = bit_table();

    // Simultaneous data copy and bit-reversal ordering into the outputs.
    for i in 0..num_samples {
        let j = fast_reverse_bits(&table, i, num_bits);
        real_out[j] = real_in[i];
        imag_out[j] = imag_in.map_or(0.0, |imag| imag[i]);
    }

    let mut block_end = 1usize;
    let mut block_size = 2usize;
    while block_size <= num_samples {
        let delta_angle = angle_numerator / block_size as f64;

        let sm2 = (-2.0 * delta_angle).sin();
        let sm1 = (-delta_angle).sin();
        let cm2 = (-2.0 * delta_angle).cos();
        let cm1 = (-delta_angle).cos();
        let w = 2.0 * cm1;

        let mut i = 0usize;
        while i < num_samples {
            let mut ar2 = cm2;
            let mut ar1 = cm1;
            let mut ai2 = sm2;
            let mut ai1 = sm1;

            let mut j = i;
            let mut n = 0usize;
            while n < block_end {
                let ar0 = w * ar1 - ar2;
                ar2 = ar1;
                ar1 = ar0;

                let ai0 = w * ai1 - ai2;
                ai2 = ai1;
                ai1 = ai0;

                let k = j + block_end;
                let tr = ar0 * real_out[k] as f64 - ai0 * imag_out[k] as f64;
                let ti = ar0 * imag_out[k] as f64 + ai0 * real_out[k] as f64;

                real_out[k] = (real_out[j] as f64 - tr) as f32;
                imag_out[k] = (imag_out[j] as f64 - ti) as f32;

                real_out[j] = (real_out[j] as f64 + tr) as f32;
                imag_out[j] = (imag_out[j] as f64 + ti) as f32;

                j += 1;
                n += 1;
            }

            i += block_size;
        }

        block_end = block_size;
        block_size <<= 1;
    }

    if inverse_transform {
        let denominator = num_samples as f32;
        for i in 0..num_samples {
            real_out[i] /= denominator;
            imag_out[i] /= denominator;
        }
    }
}

/// `NewWindowFunc` of the C++ tree: multiply `data` by the chosen window
/// function.
///
/// The C++ documentation is worth repeating: this version is symmetrical about
/// `num_samples / 2` when `extra_sample` is false and about
/// `num_samples - 1` over two otherwise, and every function but the
/// rectangular, Hamming and Gaussian ones has a zero at the start.
pub fn window_func(
    which_function: i32,
    num_samples_in: usize,
    extra_sample: bool,
    data: &mut [f32],
) {
    let mut num_samples = num_samples_in as i32;
    if extra_sample {
        debug_assert!(num_samples > 0);
        num_samples -= 1;
    }
    debug_assert!(num_samples > 0);
    debug_assert!(data.len() >= num_samples as usize);

    match which_function {
        WINDOW_BARTLETT => {
            // Bartlett (triangular) window.  Whether the length is even or
            // odd, this is the right number of pairs.
            let pairs = (num_samples - 1) / 2;
            let denominator = num_samples as f32 / 2.0;
            data[0] = 0.0;
            let mut ii = 1;
            while ii <= pairs {
                let value = ii as f32 / denominator;
                data[ii as usize] *= value;
                data[(num_samples - ii) as usize] *= value;
                ii += 1;
            }
            // For an even length the middle sample is multiplied by one, so it
            // is left alone; for an odd length one is never reached.
        }
        WINDOW_HAMMING => {
            let multiplier = 2.0 * PI / num_samples as f64;
            const COEFF0: f64 = 0.54;
            const COEFF1: f64 = -0.46;
            for ii in 0..num_samples {
                let coefficient = COEFF0 + COEFF1 * (ii as f64 * multiplier).cos();
                data[ii as usize] = (data[ii as usize] as f64 * coefficient) as f32;
            }
        }
        WINDOW_HANNING => {
            let multiplier = 2.0 * PI / num_samples as f64;
            const COEFF0: f64 = 0.5;
            const COEFF1: f64 = -0.5;
            for ii in 0..num_samples {
                let coefficient = COEFF0 + COEFF1 * (ii as f64 * multiplier).cos();
                data[ii as usize] = (data[ii as usize] as f64 * coefficient) as f32;
            }
        }
        WINDOW_BLACKMAN => {
            let multiplier = 2.0 * PI / num_samples as f64;
            let multiplier2 = 2.0 * multiplier;
            const COEFF0: f64 = 0.42;
            const COEFF1: f64 = -0.5;
            const COEFF2: f64 = 0.08;
            for ii in 0..num_samples {
                let coefficient = COEFF0
                    + COEFF1 * (ii as f64 * multiplier).cos()
                    + COEFF2 * (ii as f64 * multiplier2).cos();
                data[ii as usize] = (data[ii as usize] as f64 * coefficient) as f32;
            }
        }
        WINDOW_BLACKMAN_HARRIS => {
            let multiplier = 2.0 * PI / num_samples as f64;
            let multiplier2 = 2.0 * multiplier;
            let multiplier3 = 3.0 * multiplier;
            const COEFF0: f64 = 0.35875;
            const COEFF1: f64 = -0.48829;
            const COEFF2: f64 = 0.14128;
            const COEFF3: f64 = -0.01168;
            for ii in 0..num_samples {
                let coefficient = COEFF0
                    + COEFF1 * (ii as f64 * multiplier).cos()
                    + COEFF2 * (ii as f64 * multiplier2).cos()
                    + COEFF3 * (ii as f64 * multiplier3).cos();
                data[ii as usize] = (data[ii as usize] as f64 * coefficient) as f32;
            }
        }
        WINDOW_WELCH => {
            // Everything here is f32 in the C++.
            let n = num_samples as f32;
            for ii in 0..num_samples {
                let fraction = ii as f32 / n;
                data[ii as usize] *= 4.0 * fraction * (1.0 - fraction);
            }
        }
        WINDOW_GAUSSIAN25 | WINDOW_GAUSSIAN35 | WINDOW_GAUSSIAN45 => {
            let a = gaussian_a(which_function);
            let n = num_samples as f32;
            for ii in 0..num_samples {
                let fraction = ii as f32 / n;
                // Simplified, as the C++ comment says, from
                // exp(-0.5 * A * ((ii - N/2) / (N/2))^2).
                let exponent = a
                    * (0.25 + f64::from(fraction * fraction) - f64::from(fraction));
                data[ii as usize] = (data[ii as usize] as f64 * exponent.exp()) as f32;
            }
        }
        // The rectangular window multiplies everything by one; the C++ switch
        // prints a warning and does nothing for a selector it does not know.
        _ => {}
    }

    if extra_sample && which_function != WINDOW_RECTANGULAR {
        // For the functions that do not reach zero at the end, the extra
        // sample carries the value the first one has for the Hamming and the
        // Gaussians, and zero for the rest.
        let value = match which_function {
            WINDOW_HAMMING => 0.08,
            WINDOW_GAUSSIAN25 => (-2.0 * 2.5 * 2.5 * 0.25f64).exp(),
            WINDOW_GAUSSIAN35 => (-2.0 * 3.5 * 3.5 * 0.25f64).exp(),
            WINDOW_GAUSSIAN45 => (-2.0 * 4.5 * 4.5 * 0.25f64).exp(),
            _ => 0.0,
        };
        data[num_samples as usize] = (data[num_samples as usize] as f64 * value) as f32;
    }
}

/// `A` of the C++ derivative switch: -2a^2 for Gaussian(a).
fn gaussian_a(which_function: i32) -> f64 {
    match which_function {
        WINDOW_GAUSSIAN25 => -2.0 * 2.5 * 2.5,
        WINDOW_GAUSSIAN35 => -2.0 * 3.5 * 3.5,
        _ => -2.0 * 4.5 * 4.5,
    }
}

/// `DerivativeOfWindowFunc` of the C++ tree: multiply `data` by the
/// derivative of the chosen window function.
///
/// The C++ had the Hamming slope as a function-local static, so it was
/// initialised once, from the first call's window size, and every later call
/// at a different size used a stale slope.  Here the slope belongs to the
/// call, which is what the C++ was meant to do; the golden values were
/// captured after that was corrected on the C++ side, so the two agree.
pub fn derivative_of_window_func(
    which_function: i32,
    num_samples: usize,
    extra_sample: bool,
    data: &mut [f32],
) {
    if which_function == WINDOW_RECTANGULAR {
        // There are deltas at the ends.
        debug_assert!(num_samples > 0);
        let last = num_samples - 1;
        debug_assert!(data.len() > last);
        for ii in 1..last {
            data[ii] = 0.0;
        }
        data[last] *= -1.0;
        return;
    }

    let mut n = num_samples;
    if extra_sample {
        debug_assert!(n > 0);
        n -= 1;
    }
    debug_assert!(n > 0);
    debug_assert!(data.len() > n - 1);

    match which_function {
        WINDOW_BARTLETT => {
            // There are discontinuities in the derivative at the ends, and
            // maybe at the midpoint.  Every value here is f32 in the C++.
            let pairs = (n - 1) / 2;
            let value = 2.0f32 / n as f32;
            data[0] *= value / 2.0;
            let mut ii = 1;
            while ii <= pairs {
                data[ii as usize] *= value;
                data[n - ii] *= -value;
                ii += 1;
            }
            if n % 2 == 0 {
                data[n / 2] = 0.0;
            }
            if extra_sample {
                data[n] *= -value / 2.0;
            } else {
                // Halve the multiplier applied above.
                data[n - 1] *= 0.5;
            }
        }
        WINDOW_HAMMING => {
            // There are deltas at the ends.
            let multiplier = 2.0 * PI / n as f64;
            const COEFF0: f64 = 0.54;
            let coeff1 = -0.46 * multiplier;
            data[0] = (data[0] as f64 * COEFF0) as f32;
            if !extra_sample {
                n -= 1;
            }
            for ii in 0..n {
                let coefficient = -coeff1 * (ii as f64 * multiplier).sin();
                data[ii] = (data[ii] as f64 * coefficient) as f32;
            }
            if extra_sample {
                data[n] = (data[n] as f64 * -COEFF0) as f32;
            } else {
                // Slightly different.
                let coefficient =
                    -COEFF0 - coeff1 * (n as f64 * multiplier).sin();
                data[n] = (data[n] as f64 * coefficient) as f32;
            }
        }
        WINDOW_HANNING => {
            let multiplier = 2.0 * PI / n as f64;
            let coeff1 = -0.5 * multiplier;
            for ii in 0..n {
                let coefficient = -coeff1 * (ii as f64 * multiplier).sin();
                data[ii] = (data[ii] as f64 * coefficient) as f32;
            }
            if extra_sample {
                data[n] = 0.0;
            }
        }
        WINDOW_BLACKMAN => {
            let multiplier = 2.0 * PI / n as f64;
            let multiplier2 = 2.0 * multiplier;
            let coeff1 = -0.5 * multiplier;
            let coeff2 = 0.08 * multiplier2;
            for ii in 0..n {
                let coefficient = -coeff1 * (ii as f64 * multiplier).sin()
                    - coeff2 * (ii as f64 * multiplier2).sin();
                data[ii] = (data[ii] as f64 * coefficient) as f32;
            }
            if extra_sample {
                data[n] = 0.0;
            }
        }
        WINDOW_BLACKMAN_HARRIS => {
            let multiplier = 2.0 * PI / n as f64;
            let multiplier2 = 2.0 * multiplier;
            let multiplier3 = 3.0 * multiplier;
            let coeff1 = -0.48829 * multiplier;
            let coeff2 = 0.14128 * multiplier2;
            let coeff3 = -0.01168 * multiplier3;
            for ii in 0..n {
                let coefficient = -coeff1 * (ii as f64 * multiplier).sin()
                    - coeff2 * (ii as f64 * multiplier2).sin()
                    - coeff3 * (ii as f64 * multiplier3).sin();
                data[ii] = (data[ii] as f64 * coefficient) as f32;
            }
            if extra_sample {
                data[n] = 0.0;
            }
        }
        WINDOW_WELCH => {
            // The whole of this one is f32 in the C++, including the square
            // of the length, which is computed as an integer first.
            let n32 = n as f32;
            let nn = (n * n) as f32;
            for ii in 0..n {
                data[ii] *= 4.0 * (n32 - ii as f32 - ii as f32) / nn;
            }
            if extra_sample {
                data[n] = 0.0;
            }
            // Average the two limiting values of the discontinuous derivative.
            data[0] /= 2.0;
            data[n - 1] /= 2.0;
        }
        WINDOW_GAUSSIAN25 | WINDOW_GAUSSIAN35 | WINDOW_GAUSSIAN45 => {
            // There are deltas at the ends.
            let a = gaussian_a(which_function);
            let inv_n = 1.0f32 / n as f32;
            let inv_nn = inv_n * inv_n;
            // Simplified from the loop for ii == 0, with the term for the
            // delta added.
            data[0] = (data[0] as f64 * (((a * 0.25).exp()) * f64::from(1.0 - inv_n))) as f32;
            if !extra_sample {
                n -= 1;
            }
            for ii in 1..n {
                let fraction = ii as f32 * inv_n;
                let exponent = a
                    * (0.25 + f64::from(fraction * fraction) - f64::from(fraction));
                let coefficient = exponent.exp() * f64::from(2.0 * ii as f32 * inv_nn - inv_n);
                data[ii] = (data[ii] as f64 * coefficient) as f32;
            }
            if extra_sample {
                let coefficient = (a * 0.25).exp() * f64::from(inv_n - 1.0);
                data[n] = (data[n] as f64 * coefficient) as f32;
            } else {
                // Slightly different.
                let fraction = n as f32 * inv_n;
                let exponent = a
                    * (0.25 + f64::from(fraction * fraction) - f64::from(fraction));
                let coefficient =
                    exponent.exp() * f64::from(2.0 * n as f32 * inv_nn - inv_n - 1.0);
                data[n] = (data[n] as f64 * coefficient) as f32;
            }
        }
        // The C++ switch prints a warning and leaves the data alone.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables for a transform of `fftlen` samples.
    fn tables(fftlen: usize) -> (Vec<i32>, Vec<f32>) {
        let mut bit_reversed = vec![0i32; fftlen / 2];
        let mut sin_table = vec![0.0f32; fftlen];
        init_tables(fftlen, &mut bit_reversed, &mut sin_table);
        (bit_reversed, sin_table)
    }

    /// The input the unit tests run on: integer arithmetic narrowed to f32, so
    /// that the same values arise anywhere.
    fn input(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| ((i * 37 + 11) % 129) as f32 / 64.0 - 1.0)
            .collect()
    }

    /// The plain discrete transform, against which the ported one is checked.
    fn naive_dft(input: &[f32]) -> Vec<(f64, f64)> {
        let n = input.len();
        (0..n)
            .map(|k| {
                let mut re = 0.0;
                let mut im = 0.0;
                for (i, &x) in input.iter().enumerate() {
                    let angle = -2.0 * PI * k as f64 * i as f64 / n as f64;
                    re += f64::from(x) * angle.cos();
                    im += f64::from(x) * angle.sin();
                }
                (re, im)
            })
            .collect()
    }

    fn close(got: f32, want: f64, what: &str, at: usize) {
        let difference = (f64::from(got) - want).abs();
        assert!(
            difference <= 1e-4 * want.abs().max(1.0),
            "{} at {}: got {}, want {} (difference {})",
            what,
            at,
            got,
            want,
            difference
        );
    }

    #[test]
    fn the_bit_reversal_table_visits_every_pair_once() {
        let fftlen = 64;
        let points = fftlen / 2;
        let (bit_reversed, _) = tables(fftlen);

        // The values are offsets into the transform buffer, so they are the
        // even positions, one per pair, and every pair is used exactly once.
        let mut pairs: Vec<i32> = bit_reversed.iter().map(|at| at / 2).collect();
        for at in &bit_reversed {
            assert_eq!(at % 2, 0, "{} is not a pair offset", at);
        }
        pairs.sort_unstable();
        assert_eq!(pairs, (0..points as i32).collect::<Vec<_>>());
    }

    #[test]
    fn the_sine_table_holds_the_roots_of_unity_in_bit_reversed_order() {
        let fftlen = 16;
        let points = fftlen / 2;
        let (bit_reversed, sin_table) = tables(fftlen);

        for i in 0..points {
            let angle = 2.0 * PI * i as f64 / fftlen as f64;
            let at = bit_reversed[i] as usize;
            close(sin_table[at], -angle.sin(), "sine table", i);
            close(sin_table[at + 1], -angle.cos(), "sine table", i);
        }
    }

    #[test]
    fn a_constant_signal_transforms_to_its_length_in_dc() {
        let fftlen = 32;
        let (bit_reversed, sin_table) = tables(fftlen);
        let mut buffer = vec![1.0f32; fftlen];

        forward(&mut buffer, &bit_reversed, &sin_table);

        close(buffer[0], fftlen as f64, "DC bin", 0);
        // The Fs/2 bin rides in the imaginary half of the DC bin.
        close(buffer[1], 0.0, "Fs/2 bin", 1);
        for i in 2..fftlen {
            close(buffer[i], 0.0, "empty bin", i);
        }
    }

    #[test]
    fn the_real_transform_agrees_with_the_plain_discrete_transform() {
        let fftlen = 32;
        let points = fftlen / 2;
        let (bit_reversed, sin_table) = tables(fftlen);
        let input = input(fftlen);
        let mut real = vec![0.0f32; fftlen];
        let mut imag = vec![0.0f32; fftlen];

        real_fft(&input, &mut real, &mut imag, &bit_reversed, &sin_table);

        let dft = naive_dft(&input);
        for k in 0..=points {
            close(real[k], dft[k].0, "real part", k);
            close(imag[k], dft[k].1, "imaginary part", k);
        }
    }

    #[test]
    fn the_power_spectrum_is_the_square_of_the_magnitude() {
        let fftlen = 32;
        let (bit_reversed, sin_table) = tables(fftlen);
        let input = input(fftlen);
        let mut power = vec![0.0f32; fftlen / 2 + 1];

        power_spectrum(&input, &mut power, &bit_reversed, &sin_table);

        let dft = naive_dft(&input);
        for k in 0..=(fftlen / 2) {
            close(power[k], dft[k].0 * dft[k].0 + dft[k].1 * dft[k].1, "power", k);
        }
    }

    #[test]
    fn the_real_transform_and_its_inverse_come_back_to_the_signal() {
        let fftlen = 64;
        let (bit_reversed, sin_table) = tables(fftlen);
        let input = input(fftlen);
        let mut real = vec![0.0f32; fftlen];
        let mut imag = vec![0.0f32; fftlen];
        let mut back = vec![0.0f32; fftlen];

        real_fft(&input, &mut real, &mut imag, &bit_reversed, &sin_table);
        inverse_real_fft(&real, Some(&imag), &mut back, &bit_reversed, &sin_table);

        for i in 0..fftlen {
            close(back[i], f64::from(input[i]), "reconstructed sample", i);
        }
    }

    #[test]
    fn the_real_inverse_also_works_without_an_imaginary_part() {
        let fftlen = 64;
        let (bit_reversed, sin_table) = tables(fftlen);
        let input = input(fftlen);
        let mut real = vec![0.0f32; fftlen];
        let mut imag = vec![0.0f32; fftlen];
        let mut without = vec![0.0f32; fftlen];
        let mut with = vec![0.0f32; fftlen];

        real_fft(&input, &mut real, &mut imag, &bit_reversed, &sin_table);
        inverse_real_fft(&real, None, &mut without, &bit_reversed, &sin_table);
        inverse_real_fft(&real, Some(&imag), &mut with, &bit_reversed, &sin_table);

        // The two differ, because the imaginary parts of the spectrum are not
        // zero, but both are finite.
        for i in 0..fftlen {
            assert!(without[i].is_finite() && with[i].is_finite());
        }
    }

    #[test]
    fn the_complex_transform_agrees_with_the_plain_discrete_transform() {
        let n = 16;
        let real_in = input(n);
        let imag_in = input(n * 3);
        let mut real_out = vec![0.0f32; n];
        let mut imag_out = vec![0.0f32; n];

        fft(&real_in, Some(&imag_in), &mut real_out, &mut imag_out, false);

        let mut complex = Vec::with_capacity(n);
        let mut expected = Vec::with_capacity(n);
        for i in 0..n {
            let angle = -2.0 * PI * i as f64 / n as f64;
            complex.push((
                f64::from(real_in[i]),
                f64::from(imag_in[i]),
            ));
            expected.push((angle.cos(), angle.sin()));
        }

        for k in 0..n {
            let mut re = 0.0;
            let mut im = 0.0;
            for i in 0..n {
                let (xr, xi) = complex[i];
                let (wr, wi) = expected[(k * i) % n];
                re += xr * wr - xi * wi;
                im += xr * wi + xi * wr;
            }
            close(real_out[k], re, "real part", k);
            close(imag_out[k], im, "imaginary part", k);
        }
    }

    #[test]
    fn the_complex_transform_and_its_inverse_are_inverses() {
        let n = 64;
        let real_in = input(n);
        let imag_in = input(2 * n);
        let mut real_mid = vec![0.0f32; n];
        let mut imag_mid = vec![0.0f32; n];
        let mut real_out = vec![0.0f32; n];
        let mut imag_out = vec![0.0f32; n];

        fft(&real_in, Some(&imag_in), &mut real_mid, &mut imag_mid, false);
        fft(
            &real_mid,
            Some(&imag_mid),
            &mut real_out,
            &mut imag_out,
            true,
        );

        for i in 0..n {
            close(real_out[i], f64::from(real_in[i]), "round-tripped real", i);
            close(imag_out[i], f64::from(imag_in[i]), "round-tripped imag", i);
        }
    }

    #[test]
    fn the_bit_table_survives_being_released() {
        let n = 16;
        let real_in = input(n);
        let mut first_real = vec![0.0f32; n];
        let mut first_imag = vec![0.0f32; n];
        let mut again_real = vec![0.0f32; n];
        let mut again_imag = vec![0.0f32; n];

        fft(&real_in, None, &mut first_real, &mut first_imag, false);
        deinit_fft();
        fft(&real_in, None, &mut again_real, &mut again_imag, false);

        assert_eq!(first_real, again_real);
        assert_eq!(first_imag, again_imag);
    }

    #[test]
    fn the_rectangular_window_leaves_the_data_alone() {
        let mut data = input(64);
        let expected = data.clone();

        window_func(WINDOW_RECTANGULAR, 64, false, &mut data);

        assert_eq!(data, expected);
    }

    #[test]
    fn the_hann_window_is_zero_at_the_start_and_symmetric() {
        let n = 64;
        let mut data = vec![1.0f32; n];

        window_func(WINDOW_HANNING, n, false, &mut data);

        assert_eq!(data[0], 0.0);
        close(data[n / 2], 1.0, "centre", n / 2);
        for i in 1..n {
            close(data[i], f64::from(data[n - i]), "symmetry", i);
        }
    }

    #[test]
    fn the_extra_sample_is_the_last_one() {
        // With the extra sample the length counts it, so a length of n means a
        // window of n - 1 samples and a final one the function supplies.
        let n = 64;
        let mut data = vec![1.0f32; n];

        window_func(WINDOW_HANNING, n, true, &mut data);

        assert_eq!(data[0], 0.0);
        assert_eq!(data[n - 1], 0.0);
    }

    #[test]
    fn the_gaussian_windows_are_symmetric_and_the_spread_shapes_the_tail() {
        let n = 64;
        let mut tails = Vec::new();
        for which in [WINDOW_GAUSSIAN25, WINDOW_GAUSSIAN35, WINDOW_GAUSSIAN45] {
            let mut data = vec![1.0f32; n];

            window_func(which, n, false, &mut data);

            // The exponent is zero at the middle, so the peak is one there.
            close(data[n / 2], 1.0, "centre", n / 2);
            for i in 1..n {
                close(data[i], f64::from(data[n - i]), "symmetry", i);
            }
            tails.push(f64::from(data[0]));
        }

        // A wider spread leaves less of the window at the edges.
        assert!(
            tails[0] > tails[1] && tails[1] > tails[2],
            "tails {:?}",
            tails
        );
    }

    #[test]
    fn the_derivative_of_the_rectangular_window_is_two_deltas() {
        let n = 32;
        let mut data = input(n);

        derivative_of_window_func(WINDOW_RECTANGULAR, n, false, &mut data);

        for i in 1..(n - 1) {
            assert_eq!(data[i], 0.0, "sample {}", i);
        }
        assert_eq!(data[n - 1], -((input(n)[n - 1]) as f32));
    }

    #[test]
    fn the_derivative_of_the_bartlett_window_is_flat_between_the_ends() {
        let n = 32;
        let mut data = vec![1.0f32; n];

        derivative_of_window_func(WINDOW_BARTLETT, n, false, &mut data);

        // The slope is 2 / n, halved at the two ends.
        let value = 2.0f32 / n as f32;
        close(data[1], f64::from(value), "rising", 1);
        close(data[n - 2], f64::from(-value), "falling", n - 2);
        assert_eq!(data[n / 2], 0.0);
    }
}
