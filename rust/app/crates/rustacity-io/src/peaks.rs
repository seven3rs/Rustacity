//! A min/max peak pyramid for drawing waveforms at any zoom level.

use std::fmt;
use std::sync::Arc;

use crate::AudioFile;

/// The vertical extent of one waveform column.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Peak {
    pub min: f32,
    pub max: f32,
}

/// A multi-resolution min/max cache over the mono mixdown.
///
/// Level 0 stores one peak per `BASE_BUCKET` frames; each following level
/// merges pairs of peaks, so drawing a long file only reads a few buckets per
/// pixel. When the visible range is shorter than one base bucket the cache
/// falls back to the exact mono samples.
pub struct PeakCache {
    mono: Arc<[f32]>,
    sample_rate: u32,
    levels: Vec<Vec<Peak>>,
    base_bucket: usize,
}

const BASE_BUCKET: usize = 64;

impl PeakCache {
    pub fn new(file: &AudioFile) -> Self {
        let mut level = Vec::with_capacity(file.mono.len().div_ceil(BASE_BUCKET));

        for chunk in file.mono.chunks(BASE_BUCKET) {
            let mut min = f32::INFINITY;
            let mut max = f32::NEG_INFINITY;

            for &sample in chunk {
                min = min.min(sample);
                max = max.max(sample);
            }

            level.push(Peak { min, max });
        }

        let mut levels = vec![level];

        while levels.last().is_some_and(|level| level.len() > 1) {
            let previous = levels.last().expect("a level exists");
            let mut merged = Vec::with_capacity(previous.len().div_ceil(2));

            for pair in previous.chunks(2) {
                let min = pair
                    .iter()
                    .map(|peak| peak.min)
                    .fold(f32::INFINITY, f32::min);
                let max = pair
                    .iter()
                    .map(|peak| peak.max)
                    .fold(f32::NEG_INFINITY, f32::max);

                merged.push(Peak { min, max });
            }

            levels.push(merged);
        }

        Self {
            mono: file.mono.clone(),
            sample_rate: file.sample_rate,
            levels,
            base_bucket: BASE_BUCKET,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frames(&self) -> usize {
        self.mono.len()
    }

    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }

        self.mono.len() as f64 / f64::from(self.sample_rate)
    }

    /// Return one min/max peak for each of `width` columns covering
    /// `start_seconds..end_seconds`.
    pub fn peaks(&self, start_seconds: f64, end_seconds: f64, width: usize) -> Vec<Peak> {
        if width == 0 {
            return Vec::new();
        }

        if self.mono.is_empty() || self.sample_rate == 0 {
            return vec![Peak::default(); width];
        }

        let frames = self.mono.len();
        let rate = f64::from(self.sample_rate);

        let start = (start_seconds.max(0.0) * rate).floor().min(frames as f64) as usize;
        let end = (end_seconds.max(0.0) * rate).floor().min(frames as f64) as usize;
        let end = end.max(start);

        if start >= end {
            return vec![Peak::default(); width];
        }

        let span = end - start;
        let frames_per_pixel = span as f64 / width as f64;

        if frames_per_pixel < self.base_bucket as f64 {
            return self.exact_peaks(start, end, width);
        }

        let mut level = 0;
        let mut bucket = self.base_bucket;

        while level + 1 < self.levels.len() && (bucket * 2) as f64 <= frames_per_pixel {
            level += 1;
            bucket *= 2;
        }

        let peaks = &self.levels[level];

        if peaks.is_empty() {
            return vec![Peak::default(); width];
        }

        let mut result = Vec::with_capacity(width);

        for column in 0..width {
            let a = start + column * span / width;
            let b = (start + (column + 1) * span / width).max(a + 1).min(end);

            let first = (a / bucket).min(peaks.len() - 1);
            let last = b.div_ceil(bucket).max(first + 1).min(peaks.len());

            let mut min = f32::INFINITY;
            let mut max = f32::NEG_INFINITY;

            for peak in &peaks[first..last] {
                min = min.min(peak.min);
                max = max.max(peak.max);
            }

            result.push(Peak { min, max });
        }

        result
    }

    fn exact_peaks(&self, start: usize, end: usize, width: usize) -> Vec<Peak> {
        let span = end - start;
        let mut result = Vec::with_capacity(width);

        for column in 0..width {
            let a = start + column * span / width;
            let b = (start + (column + 1) * span / width).max(a + 1).min(end);

            let mut min = f32::INFINITY;
            let mut max = f32::NEG_INFINITY;

            for &sample in &self.mono[a..b] {
                min = min.min(sample);
                max = max.max(sample);
            }

            result.push(Peak { min, max });
        }

        result
    }
}

impl fmt::Debug for PeakCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeakCache")
            .field("frames", &self.frames())
            .field("sample_rate", &self.sample_rate)
            .field("levels", &self.levels.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_with(samples: Vec<f32>) -> PeakCache {
        let frames = samples.len();
        let file = AudioFile {
            samples: samples.clone().into(),
            mono: samples.into(),
            channels: 1,
            sample_rate: 1_000,
        };

        assert_eq!(file.frames(), frames);
        PeakCache::new(&file)
    }

    #[test]
    fn exact_peaks_cover_each_column() {
        let cache = cache_with(vec![-1.0, 0.5, -0.25, 1.0]);
        let peaks = cache.peaks(0.0, 0.004, 4);

        assert_eq!(peaks.len(), 4);
        assert_eq!(
            peaks[0],
            Peak {
                min: -1.0,
                max: -1.0
            }
        );
        assert_eq!(peaks[1], Peak { min: 0.5, max: 0.5 });
        assert_eq!(
            peaks[2],
            Peak {
                min: -0.25,
                max: -0.25
            }
        );
        assert_eq!(peaks[3], Peak { min: 1.0, max: 1.0 });
    }

    #[test]
    fn pyramid_peaks_span_the_requested_range() {
        let mut samples = vec![0.0; 256];
        samples[10] = -0.75;
        samples[200] = 0.9;

        let cache = cache_with(samples);
        let peaks = cache.peaks(0.0, 0.256, 1);

        assert_eq!(peaks.len(), 1);
        assert!((peaks[0].min + 0.75).abs() < 1e-6);
        assert!((peaks[0].max - 0.9).abs() < 1e-6);
    }

    #[test]
    fn empty_ranges_are_silent() {
        let cache = cache_with(vec![0.25, -0.25]);
        let peaks = cache.peaks(0.5, 0.6, 3);

        assert_eq!(peaks, vec![Peak::default(); 3]);
    }

    #[test]
    fn zero_width_returns_no_columns() {
        let cache = cache_with(vec![0.25]);
        assert!(cache.peaks(0.0, 0.1, 0).is_empty());
    }
}
