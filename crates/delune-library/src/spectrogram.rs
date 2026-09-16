//! A picture of a track's frequencies over time, for checking a rip by eye.
//!
//! Time runs left to right, frequency bottom to top (linear, up to Nyquist, where a
//! lossy encoder's cutoff shows as a flat ceiling), and loudness is colour. The image
//! is a PNG written without an image library: one IDAT chunk of 8-bit RGB rows.

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use rustfft::{FftPlanner, num_complex::Complex32};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::verify::VerifyError;

const WINDOW: usize = 2048;
/// Loudness below this (relative to the loudest bin) is drawn black.
const FLOOR_DB: f32 = -100.0;

/// Render `path` as a `width` × `height` PNG. CPU-heavy: run it on a blocking thread.
///
/// # Errors
///
/// When the file can't be opened or decoded.
pub fn render(path: &Path, width: usize, height: usize) -> Result<Vec<u8>, VerifyError> {
    let windows = capture(path, width)?;
    let spectra = spectra(&windows);
    let peak = spectra.iter().flatten().copied().fold(f32::MIN, f32::max).max(1e-12);
    let bins = WINDOW / 2;

    let mut pixels = Vec::with_capacity(width * height * 3);
    for row in 0..height {
        // Top row is the highest frequency.
        let bin = ((height - 1 - row) * bins) / height;
        for x in 0..width {
            let column = spectra.get(x * spectra.len().max(1) / width.max(1));
            let power = column.and_then(|c| c.get(bin)).copied().unwrap_or(0.0);
            let db = 10.0 * (power / peak).max(1e-12).log10();
            let level = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
            pixels.extend_from_slice(&colour(level));
        }
    }
    Ok(png(width, height, &pixels))
}

/// Decode the file, keeping one window of mono samples for each of `width` columns.
fn capture(path: &Path, width: usize) -> Result<Vec<Vec<f32>>, VerifyError> {
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
    let total_frames = track.num_frames;
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

    // Space the windows over the whole track when its length is known; otherwise one
    // every half second.
    let hop = total_frames.map_or(u64::from(sample_rate / 2), |n| n / width.max(1) as u64).max(WINDOW as u64);
    let mut windows: Vec<Vec<f32>> = Vec::with_capacity(width);
    let mut current: Vec<f32> = Vec::with_capacity(WINDOW);
    let mut next_at: u64 = 0;
    let mut position: u64 = 0;
    let mut interleaved: Vec<f32> = Vec::new();

    while windows.len() < width {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(VerifyError::Unplayable(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(VerifyError::Unplayable(e.to_string())),
        };
        let frames = buffer.frames() as u64;
        if position + frames < next_at && current.is_empty() {
            position += frames;
            continue;
        }
        buffer.copy_to_vec_interleaved(&mut interleaved);
        for (i, frame) in interleaved.chunks_exact(channels).enumerate() {
            if position + (i as u64) < next_at && current.is_empty() {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            current.push(frame.iter().sum::<f32>() / channels as f32);
            if current.len() == WINDOW {
                windows.push(std::mem::take(&mut current));
                current.reserve(WINDOW);
                next_at += hop;
                if windows.len() >= width {
                    break;
                }
            }
        }
        position += frames;
    }
    Ok(windows)
}

/// The power spectrum of each window.
fn spectra(windows: &[Vec<f32>]) -> Vec<Vec<f32>> {
    #[allow(clippy::cast_precision_loss)]
    let shape: Vec<f32> =
        (0..WINDOW).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos()).collect();
    let fft = FftPlanner::new().plan_fft_forward(WINDOW);
    windows
        .iter()
        .map(|samples| {
            let mut buffer: Vec<Complex32> =
                samples.iter().zip(&shape).map(|(s, w)| Complex32::new(s * w, 0.0)).collect();
            fft.process(&mut buffer);
            buffer[..WINDOW / 2].iter().map(Complex32::norm_sqr).collect()
        })
        .collect()
}

/// Black through violet and orange to pale yellow.
fn colour(level: f32) -> [u8; 3] {
    const STOPS: [(f32, [f32; 3]); 5] = [
        (0.0, [0.0, 0.0, 0.02]),
        (0.35, [0.30, 0.07, 0.45]),
        (0.6, [0.75, 0.2, 0.42]),
        (0.8, [0.98, 0.55, 0.22]),
        (1.0, [0.99, 0.99, 0.75]),
    ];
    let upper = STOPS.iter().position(|(at, _)| *at >= level).unwrap_or(STOPS.len() - 1).max(1);
    let (a, ca) = STOPS[upper - 1];
    let (b, cb) = STOPS[upper];
    let t = if b > a { (level - a) / (b - a) } else { 0.0 };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mix = |i: usize| ((ca[i] + (cb[i] - ca[i]) * t) * 255.0).round().clamp(0.0, 255.0) as u8;
    [mix(0), mix(1), mix(2)]
}

/// An 8-bit RGB PNG.
fn png(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        out.extend_from_slice(&kind);
        out.extend_from_slice(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(&kind);
        crc.update(data);
        out.extend_from_slice(&crc.finalize().to_be_bytes());
    }

    let mut raw = Vec::with_capacity((width * 3 + 1) * height);
    for row in rgb.chunks_exact(width * 3) {
        raw.push(0); // no filter
        raw.extend_from_slice(row);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = encoder.write_all(&raw);
    let compressed = encoder.finish().unwrap_or_default();

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&u32::try_from(width).unwrap_or(0).to_be_bytes());
    header.extend_from_slice(&u32::try_from(height).unwrap_or(0).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB, deflate, no filter, no interlace

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, *b"IHDR", &header);
    chunk(&mut out, *b"IDAT", &compressed);
    chunk(&mut out, *b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mono 16-bit WAV of a 1 kHz tone.
    fn tone(path: &Path, seconds: u32) {
        let rate = 44_100u32;
        let samples: Vec<i16> = (0..rate * seconds)
            .map(|i| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                let v = ((f64::from(i) * 1000.0 * std::f64::consts::TAU / f64::from(rate)).sin() * 12_000.0) as i16;
                v
            })
            .collect();
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + u32::try_from(data.len()).unwrap()).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn draws_a_tone_as_a_bright_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 3);
        let (width, height) = (40, 64);
        let image = render(&path, width, height).unwrap();
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&image[12..16], b"IHDR");

        // Decode our own output to check the tone's row is the brightest.
        let idat_start = image.windows(4).position(|w| w == b"IDAT").unwrap();
        let len = u32::from_be_bytes(image[idat_start - 4..idat_start].try_into().unwrap()) as usize;
        let mut raw = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::ZlibDecoder::new(&image[idat_start + 4..idat_start + 4 + len]),
            &mut raw,
        )
        .unwrap();
        let brightness: Vec<u32> =
            raw.chunks_exact(width * 3 + 1).map(|row| row[1..].iter().map(|&b| u32::from(b)).sum()).collect();
        let brightest = brightness.iter().enumerate().max_by_key(|(_, b)| **b).unwrap().0;
        // 1 kHz of 22.05 kHz sits about 4.5% up from the bottom.
        let expected = height - 1 - (1000 * height / 22_050);
        assert!(brightest.abs_diff(expected) <= 1, "tone at row {brightest}, expected {expected}");
    }

    #[test]
    fn refuses_what_isnt_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.flac");
        std::fs::write(&path, "not audio").unwrap();
        assert!(render(&path, 10, 10).is_err());
    }
}
