//! Does the file actually play, and is it really lossless?
//!
//! [`verify`] decodes the whole file with symphonia, counting decode errors, and
//! samples its frequency spectrum along the way.
//!
//! ## Transcode detection
//!
//! Lossy encoders throw away high frequencies: 128 kbps MP3 stops around 16 kHz,
//! 320 kbps around 20 kHz. A FLAC made from an MP3 keeps that hole. We average the
//! power spectrum of windows taken every two seconds, find the highest frequency
//! still carrying real energy relative to the midrange, and flag a lossless file
//! whose spectrum ends in a steep wall well below Nyquist.
//!
//! This is evidence, not proof: some genuine recordings have little high-frequency
//! content. delune shows the finding in review and lets the person decide.

use std::fs::File;
use std::path::Path;

use rustfft::{FftPlanner, num_complex::Complex32};
use serde::{Deserialize, Serialize};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

const WINDOW: usize = 4096;
/// Take one analysis window per this many seconds of audio.
const WINDOW_EVERY_SECS: u32 = 2;
const MAX_WINDOWS: usize = 240;
/// Energy this far below the midrange still counts as "content".
const CONTENT_DB: f32 = 60.0;
/// Above a suspected cutoff, energy must be this far below the midrange: a wall.
const WALL_DB: f32 = 80.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verification {
    pub decoded_secs: f64,
    pub sample_rate: u32,
    /// Packets that failed to decode. Anything above zero means audible damage.
    pub decode_errors: u32,
    /// Highest frequency with real content, when enough audio was analysed.
    pub cutoff_hz: Option<u32>,
    /// The spectrum ends in a wall typical of lossy encoding.
    pub suspect_transcode: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("couldn't open the file: {0}")]
    Open(#[from] std::io::Error),
    #[error("the file isn't a playable audio stream: {0}")]
    Unplayable(String),
}

/// Decode `path` completely and analyse its spectrum. CPU-heavy: run it on a
/// blocking thread.
pub fn verify(path: &Path, lossless: bool) -> Result<Verification, VerifyError> {
    let file = File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), symphonia::core::io::MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| VerifyError::Unplayable(e.to_string()))?;
    let track =
        format.default_track(TrackType::Audio).ok_or_else(|| VerifyError::Unplayable("no audio track".into()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| VerifyError::Unplayable("missing codec parameters".into()))?
        .clone();
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| VerifyError::Unplayable(e.to_string()))?;

    let sample_rate = params.sample_rate.unwrap_or(44_100);
    let channels = params.channels.as_ref().map_or(2, |c| c.count().max(1));
    let mut analyser = Analyser::new(sample_rate);
    let mut frames: u64 = 0;
    let mut decode_errors = 0u32;
    let mut interleaved: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            // End of stream, or a truncated file ending in an I/O error (which shows up
            // as missing duration).
            Ok(None) | Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(VerifyError::Unplayable(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buffer) => {
                frames += buffer.frames() as u64;
                if lossless && analyser.wants_more(frames) {
                    buffer.copy_to_vec_interleaved(&mut interleaved);
                    analyser.feed(&interleaved, channels);
                }
            }
            Err(SymphoniaError::DecodeError(_)) => decode_errors += 1,
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(VerifyError::Unplayable(e.to_string())),
        }
    }

    #[allow(clippy::cast_precision_loss)] // durations, not accounting
    let decoded_secs = frames as f64 / f64::from(sample_rate);
    let (cutoff_hz, suspect_transcode) = analyser.finish();
    Ok(Verification { decoded_secs, sample_rate, decode_errors, cutoff_hz, suspect_transcode })
}

/// Accumulates an averaged power spectrum from evenly spaced windows.
struct Analyser {
    sample_rate: u32,
    next_at_frame: u64,
    power: Vec<f32>,
    windows: usize,
    window: Vec<f32>,
    mono: Vec<f32>,
}

impl Analyser {
    fn new(sample_rate: u32) -> Self {
        // 4-term Blackman-Harris: sidelobes at -92 dB, so strong midrange content
        // doesn't leak into the empty band above a lossy cutoff and hide it.
        #[allow(clippy::cast_precision_loss)]
        let hann = (0..WINDOW)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / WINDOW as f32;
                0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
            })
            .collect();
        Self {
            sample_rate,
            // Skip the first second: fade-ins and silence say nothing about bandwidth.
            next_at_frame: u64::from(sample_rate),
            power: vec![0.0; WINDOW / 2],
            windows: 0,
            window: hann,
            mono: Vec::with_capacity(WINDOW),
        }
    }

    fn wants_more(&self, frames_so_far: u64) -> bool {
        self.windows < MAX_WINDOWS && (frames_so_far >= self.next_at_frame || !self.mono.is_empty())
    }

    fn feed(&mut self, interleaved: &[f32], channels: usize) {
        for frame in interleaved.chunks_exact(channels) {
            #[allow(clippy::cast_precision_loss)]
            self.mono.push(frame.iter().sum::<f32>() / channels as f32);
            if self.mono.len() == WINDOW {
                self.analyse_window();
                self.mono.clear();
                self.next_at_frame += u64::from(self.sample_rate * WINDOW_EVERY_SECS);
                return;
            }
        }
    }

    fn analyse_window(&mut self) {
        let energy: f32 = self.mono.iter().map(|s| s * s).sum();
        #[allow(clippy::cast_precision_loss)]
        if energy / (WINDOW as f32) < 1e-7 {
            return; // silence
        }
        let mut buffer: Vec<Complex32> =
            self.mono.iter().zip(&self.window).map(|(s, w)| Complex32::new(s * w, 0.0)).collect();
        FftPlanner::new().plan_fft_forward(WINDOW).process(&mut buffer);
        for (bin, value) in self.power.iter_mut().zip(&buffer) {
            *bin += value.norm_sqr();
        }
        self.windows += 1;
    }

    fn finish(&self) -> (Option<u32>, bool) {
        if self.windows < 3 {
            return (None, false);
        }
        #[allow(clippy::cast_precision_loss)]
        let hz_per_bin = self.sample_rate as f32 / WINDOW as f32;
        // Smooth over ~200 Hz in the power domain, then convert to dB, so gaps between
        // tones and single noisy bins don't decide the cutoff.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let span = ((200.0 / hz_per_bin) as usize).max(1);
        let smoothed: Vec<f32> = (0..self.power.len())
            .map(|i| {
                let lo = i.saturating_sub(span);
                let hi = (i + span).min(self.power.len() - 1);
                #[allow(clippy::cast_precision_loss)]
                let mean = self.power[lo..=hi].iter().sum::<f32>() / ((hi - lo + 1) * self.windows) as f32;
                10.0 * (mean + 1e-20).log10()
            })
            .collect();

        let bin = |hz: f32| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let b = (hz / hz_per_bin) as usize;
            b.min(smoothed.len() - 1)
        };
        let mut mid: Vec<f32> = smoothed[bin(1_000.0)..bin(4_000.0)].to_vec();
        mid.sort_by(f32::total_cmp);
        let reference = mid[mid.len() / 2];

        let Some(cutoff_bin) = smoothed.iter().rposition(|&level| level >= reference - CONTENT_DB) else {
            return (None, false);
        };
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cutoff_hz = (cutoff_bin as f32 * hz_per_bin) as u32;

        #[allow(clippy::cast_precision_loss)]
        let nyquist = self.sample_rate as f32 / 2.0;
        #[allow(clippy::cast_precision_loss)]
        let above = bin(cutoff_hz as f32 + 1_000.0);
        let wall = above < smoothed.len() - 1 && smoothed[above..].iter().all(|&level| level < reference - WALL_DB);
        #[allow(clippy::cast_precision_loss)]
        let suspect = wall && (cutoff_hz as f32) < (nyquist * 0.9).min(20_500.0);
        (Some(cutoff_hz), suspect)
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A 16-bit mono WAV of white noise, optionally brick-wall low-passed at
    /// `cutoff_hz` the way a lossy encoder would leave it.
    fn wav(path: &Path, cutoff_hz: Option<f32>, secs: u32) {
        let rate = 44_100u32;
        let frames = (rate * secs) as usize;
        let mut seed = 0x2545_f491_u32;
        let mut noise: Vec<Complex32> = (0..frames)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                Complex32::new(seed as f32 / u32::MAX as f32 - 0.5, 0.0)
            })
            .collect();
        if let Some(cutoff) = cutoff_hz {
            let mut planner = FftPlanner::new();
            planner.plan_fft_forward(frames).process(&mut noise);
            let hz_per_bin = rate as f32 / frames as f32;
            for (i, bin) in noise.iter_mut().enumerate() {
                let hz = i.min(frames - i) as f32 * hz_per_bin;
                if hz > cutoff {
                    *bin = Complex32::new(0.0, 0.0);
                }
            }
            planner.plan_fft_inverse(frames).process(&mut noise);
            for bin in &mut noise {
                *bin /= frames as f32;
            }
        }
        let peak = noise.iter().map(|c| c.re.abs()).fold(0.0f32, f32::max);
        let mut data = Vec::with_capacity(frames * 2);
        for sample in &noise {
            data.extend_from_slice(&((sample.re / peak * 24_000.0) as i16).to_le_bytes());
        }
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data.len() as u32).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        file.write_all(&1u16.to_le_bytes()).unwrap(); // mono
        file.write_all(&rate.to_le_bytes()).unwrap();
        file.write_all(&(rate * 2).to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        file.write_all(&data).unwrap();
    }

    #[test]
    fn full_band_audio_is_not_suspect() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full.wav");
        wav(&path, None, 12);
        let result = verify(&path, true).unwrap();
        assert!((result.decoded_secs - 12.0).abs() < 0.01, "{result:?}");
        assert_eq!(result.decode_errors, 0);
        assert!(result.cutoff_hz.unwrap() > 20_500, "{result:?}");
        assert!(!result.suspect_transcode, "{result:?}");
    }

    #[test]
    fn spectrum_ending_at_16khz_is_suspect() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lowpassed.wav");
        wav(&path, Some(16_000.0), 12);
        let result = verify(&path, true).unwrap();
        let cutoff = result.cutoff_hz.unwrap();
        assert!((15_500..17_000).contains(&cutoff), "{result:?}");
        assert!(result.suspect_transcode, "{result:?}");
    }

    #[test]
    fn garbage_is_unplayable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake.flac");
        std::fs::write(&path, b"this is not audio at all").unwrap();
        assert!(matches!(verify(&path, true), Err(VerifyError::Unplayable(_))));
    }
}
