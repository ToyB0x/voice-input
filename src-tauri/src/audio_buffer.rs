pub const MAX_SECONDS: u32 = 60;

/// Mono PCM kept only in memory. No audio is written to disk or serialized to IPC.
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
    pub level: f32,
    limit: usize,
}

impl AudioBuffer {
    pub fn new(sample_rate: u32, channels: usize) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&sample_rate) || !(1..=32).contains(&channels) {
            return Err("対応していない入力デバイスの形式です。".into());
        }
        let limit = sample_rate as usize * MAX_SECONDS as usize;
        Ok(Self {
            samples: Vec::with_capacity(limit),
            sample_rate,
            channels,
            level: 0.0,
            limit,
        })
    }

    pub fn push(&mut self, interleaved: impl Iterator<Item = f32>) {
        let mut sum = 0.0;
        let mut channels = 0;
        let mut energy = 0.0;
        let mut frames = 0;
        for sample in interleaved {
            if self.is_full() {
                break;
            }
            sum += if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            channels += 1;
            if channels == self.channels {
                let mono = sum / self.channels as f32;
                self.samples.push(mono);
                energy += mono * mono;
                frames += 1;
                channels = 0;
                sum = 0.0;
            }
        }
        self.level = if frames > 0 {
            (energy / frames as f32).sqrt().min(1.0)
        } else {
            0.0
        };
    }

    pub fn is_full(&self) -> bool {
        self.samples.len() == self.limit
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.samples.len() as u64 * 1_000 / u64::from(self.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_is_downmixed_and_duration_counts_frames() {
        let mut buffer = AudioBuffer::new(8_000, 2).unwrap();
        buffer.push([0.25, 0.75].into_iter().cycle().take(16_000));
        assert_eq!(buffer.samples, vec![0.5; 8_000]);
        assert_eq!(buffer.elapsed_ms(), 1_000);
        assert!((buffer.level - 0.5).abs() < 0.001);
    }

    #[test]
    fn generated_tone_has_expected_level() {
        let mut buffer = AudioBuffer::new(48_000, 1).unwrap();
        buffer.push(
            (0..48_000).map(|i| (i as f32 * std::f32::consts::TAU * 440.0 / 48_000.0).sin() * 0.5),
        );
        assert!((buffer.level - 0.5 / 2.0_f32.sqrt()).abs() < 0.001);
        assert_eq!(buffer.elapsed_ms(), 1_000);
    }

    #[test]
    fn silence_and_non_finite_samples_are_safe() {
        let mut buffer = AudioBuffer::new(8_000, 1).unwrap();
        buffer.push([f32::NAN, f32::INFINITY, 0.0].into_iter());
        assert_eq!(buffer.samples, [0.0, 0.0, 0.0]);
        assert_eq!(buffer.level, 0.0);
        buffer.push([2.0, -2.0].into_iter());
        assert_eq!(&buffer.samples[3..], &[1.0, -1.0]);
    }

    #[test]
    fn recording_cannot_exceed_memory_limit() {
        let mut buffer = AudioBuffer::new(8_000, 1).unwrap();
        buffer.push(std::iter::repeat_n(0.5, 8_000 * 61));
        assert!(buffer.is_full());
        assert_eq!(buffer.elapsed_ms(), 60_000);
        buffer.push([1.0; 10].into_iter());
        assert_eq!(buffer.samples.len(), 480_000);
    }

    #[test]
    fn invalid_device_formats_are_rejected() {
        for (rate, channels) in [(0, 1), (384_000, 2), (48_000, 0), (48_000, 33)] {
            assert!(AudioBuffer::new(rate, channels).is_err());
        }
    }
}
