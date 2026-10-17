// Rustacity: A Digital Audio Editor
//
// Copyright (C) 2026 Sadik Pranto
//
// SPDX-License-Identifier: GPL-2.0-or-later

//! The LSAR audio interpolation the Repair effect uses, and the linear algebra
//! it is built on.
//!
//! Faithful port of the original C++ implementation by Dominic Mazzoni
//! (`src/InterpolateAudioMath.{h,cpp}`, the arithmetic that used to sit in
//! `src/InterpolateAudio.cpp` and `src/Matrix.{h,cpp}`): a least-squares
//! autoregression over the good samples on either side of a run of bad ones,
//! as described in Godsill, Rayner and Cappe, *Digital Audio Restoration*, and
//! solved by Gaussian elimination, with a plain linear interpolation as the
//! fallback whenever there is too little good data to work with or a matrix
//! turns out to be singular.
//!
//! Every expression is transcribed in the same order and precision as the C++,
//! so the golden values in `rust/dsp/goldens/reference.txt` pin the results
//! including the rounding.  The float buffers are widened to `f64` for the
//! solve and narrowed back, exactly as the C++ `Vector s(len, buffer)` does.
//!
//! Two things are deliberately not the same as the C++:
//!
//! * The trace of noise the original adds to keep the matrices away from
//!   singularity, about one bit in 16-bit audio, came from the process-global
//!   C library `rand()`, which the application seeds from the clock and other
//!   code consumes, so its values changed from run to run.  [`GlibcRand`]
//!   reproduces glibc's generator and starts it at 1, so the same call gives
//!   the same answer every time and on every platform.
//! * The linear fallback's last branch reads the sample before the buffer in
//!   the C++ when the bad run starts at sample 1 of a two-sample buffer, which
//!   is out of bounds.  It reads zero here.
//!
//! Contributor: Sadik Pranto <pranto.smss.dev@gmail.com>

/// `RAND_MAX` of glibc, whose generator is the one the port reproduces.  It is
/// 32767 on Windows and 2147483647 elsewhere, which is why the original's noise
/// was a different size on every platform; this port is the same everywhere.
const RAND_MAX: i32 = 2147483647;

/// The generator the noise comes from: glibc's `rand()`.
///
/// Public because the golden test pins the values it hands out against the
/// sequence the capture recorded, which is what makes them the right ones.
///
/// glibc's generator is the additive feedback one behind `random()`: a cycle
/// of 31 words with `x[n] = x[n-3] + x[n-31]` and the low bit of each result
/// discarded, seeded by the 16807 multiplicative recurrence, after which the
/// first 310 results are thrown away.
pub struct GlibcRand {
    state: [i32; 31],
    front: usize,
    rear: usize,
}

impl GlibcRand {
    /// A generator with `seed` in the state the C library starts from.  A seed
    /// of zero means one, as it does in glibc.
    pub fn new(seed: u32) -> GlibcRand {
        let seed = if seed == 0 { 1 } else { seed };
        let mut state = [0i32; 31];
        state[0] = seed as i32;
        // state[i] = (16807 * state[i - 1]) % 2147483647, in the form that
        // keeps the intermediate product inside a signed 32-bit word.
        let mut word = i64::from(seed);
        for slot in state.iter_mut().skip(1) {
            let high = word / 127773;
            let low = word % 127773;
            word = 16807 * low - 2836 * high;
            if word < 0 {
                word += 2147483647;
            }
            *slot = word as i32;
        }

        let mut rand = GlibcRand {
            state,
            front: 3,
            rear: 0,
        };
        for _ in 0..310 {
            rand.next();
        }
        rand
    }

    /// The next value, in `0..=RAND_MAX`.
    pub fn next(&mut self) -> i32 {
        let sum = self.state[self.front].wrapping_add(self.state[self.rear]);
        self.state[self.front] = sum;
        let value = (sum as u32 >> 1) as i32;
        self.front += 1;
        if self.front >= self.state.len() {
            self.front = 0;
            self.rear += 1;
        } else {
            self.rear += 1;
            if self.rear >= self.state.len() {
                self.rear = 0;
            }
        }
        value
    }
}

/// A dense row-major matrix, the shape the C++ `Matrix` carried as one `Vector`
/// per row.
#[derive(Clone)]
struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

impl Matrix {
    /// The C++ constructor zeroes a matrix it is not given the data for.
    fn new(rows: usize, cols: usize) -> Matrix {
        Matrix {
            rows,
            cols,
            data: vec![0.0; rows * cols],
        }
    }

    fn at(&self, row: usize, col: usize) -> f64 {
        self.data[row * self.cols + col]
    }

    fn at_mut(&mut self, row: usize, col: usize) -> &mut f64 {
        &mut self.data[row * self.cols + col]
    }

    fn swap_rows(&mut self, a: usize, b: usize) {
        for col in 0..self.cols {
            self.data.swap(a * self.cols + col, b * self.cols + col);
        }
    }
}

fn identity(n: usize) -> Matrix {
    let mut m = Matrix::new(n, n);
    for i in 0..n {
        *m.at_mut(i, i) = 1.0;
    }
    m
}

fn transpose(m: &Matrix) -> Matrix {
    let mut out = Matrix::new(m.cols, m.rows);
    for i in 0..m.rows {
        for j in 0..m.cols {
            *out.at_mut(j, i) = m.at(i, j);
        }
    }
    out
}

fn matrix_multiply(left: &Matrix, right: &Matrix) -> Matrix {
    assert_eq!(left.cols, right.rows);
    let mut out = Matrix::new(left.rows, right.cols);
    for i in 0..left.rows {
        for j in 0..right.cols {
            let mut sum = 0.0;
            for k in 0..left.cols {
                sum += left.at(i, k) * right.at(k, j);
            }
            *out.at_mut(i, j) = sum;
        }
    }
    out
}

fn matrix_subset(
    m: &Matrix,
    start_row: usize,
    num_rows: usize,
    start_col: usize,
    num_cols: usize,
) -> Matrix {
    let mut out = Matrix::new(num_rows, num_cols);
    for i in 0..num_rows {
        for j in 0..num_cols {
            *out.at_mut(i, j) = m.at(start_row + i, start_col + j);
        }
    }
    out
}

fn matrix_concatenate_cols(left: &Matrix, right: &Matrix) -> Matrix {
    assert_eq!(left.rows, right.rows);
    let mut out = Matrix::new(left.rows, left.cols + right.cols);
    for i in 0..left.rows {
        for j in 0..left.cols {
            *out.at_mut(i, j) = left.at(i, j);
        }
        for j in 0..right.cols {
            *out.at_mut(i, j + left.cols) = right.at(i, j);
        }
    }
    out
}

fn matrix_scalar_mul(m: &Matrix, factor: f64) -> Matrix {
    let mut out = Matrix::new(m.rows, m.cols);
    for i in 0..m.rows {
        for j in 0..m.cols {
            *out.at_mut(i, j) = m.at(i, j) * factor;
        }
    }
    out
}

/// `operator*(const Matrix &, const Vector &)`: the dot products are summed in
/// the order the C++ summed them, which is what keeps the rounding the same.
fn matrix_mul_vector(m: &Matrix, v: &[f64]) -> Vec<f64> {
    assert_eq!(m.cols, v.len());
    let mut out = vec![0.0; m.rows];
    for i in 0..m.rows {
        let mut sum = 0.0;
        for j in 0..m.cols {
            sum += m.at(i, j) * v[j];
        }
        out[i] = sum;
    }
    out
}

fn vector_subset(v: &[f64], start: usize, len: usize) -> Vec<f64> {
    v[start..start + len].to_vec()
}

fn vector_concatenate(left: &[f64], right: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(left.len() + right.len());
    out.extend_from_slice(left);
    out.extend_from_slice(right);
    out
}

/// Gauss-Jordan elimination with partial pivoting: the C++ `InvertMatrix`.
/// `None` is the C++ `false` - a singular matrix, and the caller falls back on
/// the linear interpolation.
fn invert(input: &Matrix) -> Option<Matrix> {
    assert_eq!(input.rows, input.cols);
    let n = input.rows;

    let mut m = input.clone();
    let mut minv = identity(n);

    for i in 0..n {
        // Pivot the row with the largest absolute value in column i into row i.
        let mut absmax = 0.0;
        let mut argmax = 0;
        for j in i..n {
            if m.at(j, i).abs() > absmax {
                absmax = m.at(j, i).abs();
                argmax = j;
            }
        }

        if absmax == 0.0 {
            return None;
        }

        if i != argmax {
            m.swap_rows(i, argmax);
            minv.swap_rows(i, argmax);
        }

        // Divide this row by the value of its diagonal entry.
        let factor = 1.0 / m.at(i, i);
        for col in 0..n {
            *m.at_mut(i, col) *= factor;
        }
        for col in 0..n {
            *minv.at_mut(i, col) *= factor;
        }

        // Eliminate the rest of the column.
        for j in 0..n {
            if j == i {
                continue;
            }
            if m.at(j, i).abs() > 0.0 {
                let factor = m.at(j, i);
                for k in 0..n {
                    let m_ik = m.at(i, k);
                    *m.at_mut(j, k) -= m_ik * factor;
                    let minv_ik = minv.at(i, k);
                    *minv.at_mut(j, k) -= minv_ik * factor;
                }
            }
        }
    }

    Some(minv)
}

/// Interpolate `num_bad` samples of `buffer` from `first_bad`, taking their
/// values from the samples around the run.
///
/// The C++ asserts that the run is inside the buffer and is not the whole of
/// it; a call that would read outside the buffer does nothing here rather than
/// reading whatever is there.  A run of no samples does nothing either, which
/// is what the C++ did after its fallback had decided there was nothing to
/// fill.
pub fn interpolate_audio(buffer: &mut [f32], first_bad: usize, num_bad: usize) {
    let len = buffer.len();
    if num_bad == 0 || num_bad >= len || first_bad >= len || num_bad > len - first_bad {
        return;
    }

    // The C++ drew the noise from the process-global generator; this one is
    // ours and always starts at the same place.
    let mut rand = GlibcRand::new(1);
    interpolate(buffer, first_bad, num_bad, &mut rand);
}

fn interpolate(buffer: &mut [f32], first_bad: usize, num_bad: usize, rand: &mut GlibcRand) {
    let len = buffer.len();

    if first_bad == 0 {
        // The algorithm has a weird asymmetry in that it performs poorly when
        // interpolating to the left, so a run at the start of the buffer is
        // reversed, interpolated as a run at the end, and reversed back.  The
        // generator carries on across the reversal, as it did in the C++.
        let mut reversed: Vec<f32> = (0..len).map(|i| buffer[len - 1 - i]).collect();
        interpolate(&mut reversed, len - num_bad, num_bad, rand);
        for i in 0..len {
            buffer[len - 1 - i] = reversed[i];
        }
        return;
    }

    // The signal the solve works on, widened to f64 as the C++ `Vector` was.
    let mut s: Vec<f64> = buffer.iter().map(|&x| f64::from(x)).collect();

    // The order of the autoregression: three samples for each bad one, at most
    // 50, and never more than the good samples available on either side.
    let order = {
        let three = (num_bad * 3) as i64;
        let before = first_bad as i64 - 1;
        let after = len as i64 - (first_bad + num_bad) as i64 - 1;
        three.min(50).min(before.max(after))
    };

    if order < 3 || order as usize >= len {
        linear_interpolate_audio(buffer, first_bad, num_bad);
        return;
    }

    let p = order as usize;

    // A tiny amount of noise on the input, about 1 bit in 16-bit audio, which
    // is an extremely effective way to avoid nearly-singular matrices.  In the
    // C++ the amount also depended on the platform's RAND_MAX; here it does not.
    for value in s.iter_mut() {
        *value += f64::from(rand.next() - RAND_MAX / 2) / (f64::from(RAND_MAX) * 10000.0);
    }

    // Fit the autoregression coefficients to all of the good data, by least
    // squares.
    let mut x = Matrix::new(p, p);
    let mut b = vec![0.0; p];
    for i in 0..len {
        if i + p < len && (i + p < first_bad || i >= first_bad + num_bad) {
            for row in 0..p {
                for col in 0..p {
                    let product = s[i + row] * s[i + col];
                    *x.at_mut(row, col) += product;
                }
                b[row] += s[i + p] * s[i + row];
            }
        }
    }

    let xinv = match invert(&x) {
        Some(inverted) => inverted,
        // The matrix is singular.  With the noise added this is not expected to
        // happen, but the original falls back on the linear interpolation and
        // so does this.
        None => {
            linear_interpolate_audio(buffer, first_bad, num_bad);
            return;
        }
    };

    // The autoregression coefficients.
    let a = matrix_mul_vector(&xinv, &b);

    // A Toeplitz matrix, encoding the autoregressive relationship between
    // elements of the sequence.
    let mut toeplitz = Matrix::new(len - p, len);
    for row in 0..len - p {
        for col in 0..p {
            *toeplitz.at_mut(row, row + col) = -a[col];
        }
        *toeplitz.at_mut(row, row + p) = 1.0;
    }

    // Split both it and the signal into the unknown (bad) samples, "u", and the
    // known (good) ones, "k".  The code assumes the bad samples are contiguous.
    let au = matrix_subset(&toeplitz, 0, len - p, first_bad, num_bad);
    let a_left = matrix_subset(&toeplitz, 0, len - p, 0, first_bad);
    let a_right = matrix_subset(
        &toeplitz,
        0,
        len - p,
        first_bad + num_bad,
        len - (first_bad + num_bad),
    );
    let ak = matrix_concatenate_cols(&a_left, &a_right);

    let s_left = vector_subset(&s, 0, first_bad);
    let s_right = vector_subset(&s, first_bad + num_bad, len - (first_bad + num_bad));
    let sk = vector_concatenate(&s_left, &s_right);

    // The least-squares solution for the unknown samples.
    let au_t = transpose(&au);
    let x1 = matrix_multiply(&au_t, &au);
    let x2 = match invert(&x1) {
        Some(inverted) => inverted,
        None => {
            linear_interpolate_audio(buffer, first_bad, num_bad);
            return;
        }
    };
    let x2b = matrix_scalar_mul(&x2, -1.0);
    let x3 = matrix_multiply(&x2b, &au_t);
    let x4 = matrix_multiply(&x3, &ak);
    let su = matrix_mul_vector(&x4, &sk);

    for i in 0..num_bad {
        buffer[first_bad + i] = su[i] as f32;
    }
}

/// The interpolation the LSAR path falls back on when there is not enough good
/// data, or when a matrix under it turns out to be singular.
///
/// The C++'s first branch, for a run at the start of the buffer, is kept for the
/// same reason it is still in the C++: nothing reaches it, because the entry
/// point reverses such a run, and it is part of what the original did.
fn linear_interpolate_audio(buffer: &mut [f32], first_bad: usize, num_bad: usize) {
    let len = buffer.len();
    let decay = 0.9f32;

    if first_bad == 0 {
        // If the bad samples are at the start, decay them back to zero.
        let mut delta = buffer[num_bad] - buffer[num_bad + 1];
        let mut value = buffer[num_bad];
        let mut i = num_bad as isize - 1;
        while i >= 0 {
            value += delta;
            buffer[i as usize] = value;
            value *= decay;
            delta *= decay;
            i -= 1;
        }
    } else if first_bad + num_bad == len {
        // If they are at the end, decay them back to zero as well.  The C++
        // read the sample before the buffer when the run starts at sample 1;
        // there is nothing before it, so this reads zero.
        let before = if first_bad >= 2 {
            buffer[first_bad - 2]
        } else {
            0.0
        };
        let mut delta = buffer[first_bad - 1] - before;
        let mut value = buffer[first_bad - 1];
        let mut i = first_bad;
        while i < first_bad + num_bad {
            value += delta;
            buffer[i] = value;
            value *= decay;
            delta *= decay;
            i += 1;
        }
    } else {
        // In the middle it is literally linear.
        let v1 = buffer[first_bad - 1];
        let v2 = buffer[first_bad + num_bad];
        let mut value = v1;
        let delta = (v2 - v1) / (num_bad + 1) as f32;
        let mut i = first_bad;
        while i < first_bad + num_bad {
            value += delta;
            buffer[i] = value;
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The samples the unit tests run on: integer arithmetic narrowed to `f32`,
    /// the same shape the dumper uses, so that the same values arise anywhere.
    fn input(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| ((i * 37) % 129) as f32 / 64.0 - 1.0)
            .collect()
    }

    #[test]
    fn the_generator_reproduces_the_c_sequences_first_values() {
        // The values glibc's rand() hands out from seed 1: the first ten, which
        // are the same on every platform that has glibc, and the ones a
        // Windows build would have given are the point of not using them.
        let mut rand = GlibcRand::new(1);
        let first: Vec<i32> = (0..10).map(|_| rand.next()).collect();
        assert_eq!(
            first,
            vec![
                1804289383, 846930886, 1681692777, 1714636915, 1957747793, 424238335, 719885386,
                1649760492, 596516649, 1189641421
            ]
        );
    }

    #[test]
    fn the_generator_stays_within_rand_max_and_repeats_itself() {
        let mut rand = GlibcRand::new(1);
        let mut other = GlibcRand::new(1);
        for _ in 0..10000 {
            let value = rand.next();
            assert!(value >= 0 && value <= RAND_MAX);
            assert_eq!(value, other.next());
        }
    }

    #[test]
    fn a_matrix_inverted_times_itself_is_the_identity() {
        // A matrix with a nonzero pivot only on the diagonal, so that the
        // elimination has to pivot, and the inverse is known in closed form.
        let mut m = Matrix::new(3, 3);
        *m.at_mut(0, 0) = 2.0;
        *m.at_mut(1, 1) = 4.0;
        *m.at_mut(2, 2) = 0.5;

        let inverted = invert(&m).expect("a matrix with no zero on the diagonal");
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                let got = (0..3).map(|k| m.at(i, k) * inverted.at(k, j)).sum::<f64>();
                assert!((got - expected).abs() < 1e-12, "at {} {}: {}", i, j, got);
            }
        }
    }

    #[test]
    fn a_singular_matrix_is_refused() {
        // The second row is twice the first, so the elimination finds no pivot
        // in the second column and the caller has to fall back.
        let mut m = Matrix::new(2, 2);
        *m.at_mut(0, 0) = 1.0;
        *m.at_mut(0, 1) = 2.0;
        *m.at_mut(1, 0) = 2.0;
        *m.at_mut(1, 1) = 4.0;
        assert!(invert(&m).is_none());
    }

    #[test]
    fn a_run_of_bad_samples_is_filled_in_and_the_rest_is_untouched() {
        let mut buffer = input(64);
        let before = buffer.clone();

        interpolate_audio(&mut buffer, 20, 5);

        for (i, (&was, &now)) in before.iter().zip(buffer.iter()).enumerate() {
            if i < 20 || i >= 25 {
                assert_eq!(was, now, "sample {} should not have changed", i);
            } else {
                assert_ne!(was, now, "sample {} should have been interpolated", i);
                assert!(now.abs() < 2.0, "sample {} is not plausible: {}", i, now);
            }
        }
    }

    #[test]
    fn a_run_at_the_start_is_filled_in_too() {
        let mut buffer = input(64);
        let before = buffer.clone();

        interpolate_audio(&mut buffer, 0, 8);

        for i in 8..64 {
            assert_eq!(before[i], buffer[i], "sample {} should not have changed", i);
        }
        for i in 0..8 {
            assert_ne!(before[i], buffer[i], "sample {} should have been filled", i);
        }
    }

    #[test]
    fn a_run_with_too_little_data_around_it_is_filled_in_anyway() {
        // Both sides are short enough that the order comes out below three, so
        // this is the linear fallback rather than the solve.
        let mut buffer = input(6);
        interpolate_audio(&mut buffer, 2, 2);

        let (v1, v2) = (buffer[1], buffer[4]);
        let (low, high) = (v1.min(v2), v1.max(v2));
        for i in 2..=3 {
            assert!(
                buffer[i] > low && buffer[i] < high,
                "sample {} is not between the samples it was interpolated from: {}",
                i,
                buffer[i]
            );
        }
    }

    #[test]
    fn the_same_call_gives_the_same_answer_every_time() {
        // The noise used to come from the process-global generator, so two
        // calls gave two answers; here the generator starts at the same place.
        let mut first = input(64);
        let mut second = input(64);
        interpolate_audio(&mut first, 20, 5);
        interpolate_audio(&mut second, 20, 5);
        assert_eq!(first, second);
    }

    #[test]
    fn a_call_that_would_read_outside_the_buffer_does_nothing() {
        let mut buffer = input(8);
        let before = buffer.clone();

        interpolate_audio(&mut buffer, 4, 8); // the run is the whole buffer
        interpolate_audio(&mut buffer, 8, 1); // it starts past the end
        interpolate_audio(&mut buffer, 6, 4); // it runs past the end
        interpolate_audio(&mut buffer, 4, 0); // there is nothing to fill

        assert_eq!(buffer, before);
    }
}
