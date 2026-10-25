use crate::{Error, MAX_SAMPLES, SAMPLE_RATE};
use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Band-limited conversion from device-rate mono PCM. Sub-second clips are
/// padded with silence to meet Whisper's minimum; input never exceeds 60 s.
pub fn prepare_audio(audio: &[f32], rate: u32) -> Result<Vec<f32>, Error> {
    if !(8_000..=192_000).contains(&rate)
        || audio.is_empty()
        || audio.len() > rate as usize * 60
        || audio.iter().any(|s| !s.is_finite() || s.abs() > 1.0)
    {
        return Err(Error::InvalidAudio);
    }
    let length = (audio.len() * SAMPLE_RATE as usize / rate as usize).min(MAX_SAMPLES);
    let mut output = if rate == SAMPLE_RATE {
        audio.to_vec()
    } else {
        let mut converter = Fft::<f32>::new(
            rate as usize,
            SAMPLE_RATE as usize,
            1024,
            1,
            FixedSync::Both,
        )
        .map_err(|e| Error::Inference(format!("resampling: {e}")))?;
        let input = InterleavedSlice::new(audio, 1, audio.len())
            .map_err(|e| Error::Inference(format!("resampling: {e}")))?;
        // rubato 5's whole-clip API drains the filter and trims its startup delay.
        let mut converted = converter
            .process_all(&input, audio.len(), None)
            .map_err(|e| Error::Inference(format!("resampling: {e}")))?
            .take_data();
        converted.truncate(length);
        converted
    };
    // FFT ringing can overshoot normalized input, especially at discontinuities.
    for sample in &mut output {
        *sample = sample.clamp(-1.0, 1.0);
    }
    output.resize(length.max(SAMPLE_RATE as usize), 0.0);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tone(rate: u32, hz: f32) -> Vec<f32> {
        (0..rate)
            .map(|n| 0.5 * (n as f32 * std::f32::consts::TAU * hz / rate as f32).sin())
            .collect()
    }
    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }
    #[test]
    fn preserves_duration_and_speech_band_at_device_rates() {
        for rate in [8_000, 16_000, 44_100, 48_000, 192_000] {
            let output = prepare_audio(&tone(rate, 440.0), rate).unwrap();
            assert_eq!(output.len(), 16_000);
            assert!((rms(&output[500..15_500]) - 0.5 / 2_f32.sqrt()).abs() < 0.01);
        }
    }
    #[test]
    fn rejects_aliasing_above_target_nyquist() {
        let output = prepare_audio(&tone(48_000, 12_000.0), 48_000).unwrap();
        assert!(rms(&output[500..15_500]) < 0.005);
    }
    #[test]
    fn pads_short_clips_and_rejects_invalid_input() {
        assert_eq!(
            prepare_audio(&vec![0.0; 4_800], 48_000).unwrap(),
            vec![0.0; 16_000]
        );
        for (audio, rate) in [
            (vec![], 16_000),
            (vec![f32::NAN], 48_000),
            (vec![0.0], 0),
            (vec![0.0; 480_001], 8_000),
        ] {
            assert!(prepare_audio(&audio, rate).is_err());
        }
    }
}
