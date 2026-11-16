use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct Volume(Arc<AtomicU8>);

impl Volume {
    pub fn new(value: u8) -> Self {
        Self(Arc::new(AtomicU8::new(value)))
    }

    pub fn load(&self) -> u8 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn store(&self, value: u8) {
        self.0.store(value, Ordering::Relaxed);
    }
}

pub struct PcmScaler {
    bytes_per_sample: usize,
    pending: Vec<u8>,
    output: Vec<u8>,
}

impl PcmScaler {
    pub fn new(bits_per_sample: u16) -> Self {
        assert!([8, 16, 24, 32].contains(&bits_per_sample));
        Self {
            bytes_per_sample: usize::from(bits_per_sample / 8),
            pending: Vec::new(),
            output: Vec::new(),
        }
    }

    pub fn process(&mut self, input: &[u8], volume: u8) -> &[u8] {
        assert!(volume <= 100);
        self.pending.extend_from_slice(input);
        // ESP32 swaps pairs of mono 8- and 16-bit samples on the wire.
        let group_size = if self.bytes_per_sample <= 2 {
            self.bytes_per_sample * 2
        } else {
            self.bytes_per_sample
        };
        let complete = self.pending.len() / group_size * group_size;
        self.output.clear();
        for group in self.pending[..complete].chunks_exact(group_size) {
            if self.bytes_per_sample <= 2 {
                Self::append_sample(&mut self.output, &group[self.bytes_per_sample..], volume);
                Self::append_sample(&mut self.output, &group[..self.bytes_per_sample], volume);
            } else {
                Self::append_sample(&mut self.output, group, volume);
            }
        }
        self.pending.drain(..complete);
        &self.output
    }

    pub fn has_partial_sample(&self) -> bool {
        self.pending.len() % self.bytes_per_sample != 0
    }

    pub fn finish(&mut self, volume: u8) -> &[u8] {
        assert!(!self.has_partial_sample());
        self.output.clear();
        if !self.pending.is_empty() {
            self.output.extend_from_slice(&[0, 0]);
            Self::append_sample(&mut self.output, &self.pending, volume);
            self.pending.clear();
        }
        &self.output
    }

    fn append_sample(output: &mut Vec<u8>, sample: &[u8], volume: u8) {
        let width = sample.len();
        let value = if width == 1 {
            i64::from(sample[0]) - 128
        } else {
            let sign = if sample[width - 1] & 0x80 == 0 {
                0
            } else {
                0xff
            };
            let mut bytes = [sign; 4];
            bytes[..width].copy_from_slice(sample);
            i64::from(i32::from_le_bytes(bytes))
        };
        let scaled = (value * i64::from(volume) / 100) as i32;
        // ESP32 expects 8- and 24-bit values in the high bits of 16- and 32-bit words.
        if width == 1 || width == 3 {
            output.push(0);
        }
        output.extend_from_slice(&scaled.to_le_bytes()[..width]);
    }
}

#[cfg(test)]
mod tests {
    use super::PcmScaler;

    #[test]
    fn scales_each_pcm_width_into_esp32_i2s_format() {
        for (bits, samples, full, half, silent) in [
            (
                8,
                vec![0, 255],
                vec![0, 127, 0, 128],
                vec![0, 63, 0, 192],
                vec![0; 4],
            ),
            (
                16,
                vec![0x00, 0x80, 0xff, 0x7f],
                vec![0xff, 0x7f, 0x00, 0x80],
                vec![0xff, 0x3f, 0x00, 0xc0],
                vec![0; 4],
            ),
            (
                24,
                vec![0, 0, 0x80, 0xff, 0xff, 0x7f],
                vec![0, 0, 0, 0x80, 0, 0xff, 0xff, 0x7f],
                vec![0, 0, 0, 0xc0, 0, 0xff, 0xff, 0x3f],
                vec![0; 8],
            ),
            (
                32,
                vec![0, 0, 0, 0x80, 0xff, 0xff, 0xff, 0x7f],
                vec![0, 0, 0, 0x80, 0xff, 0xff, 0xff, 0x7f],
                vec![0, 0, 0, 0xc0, 0xff, 0xff, 0xff, 0x3f],
                vec![0; 8],
            ),
        ] {
            let mut scaler = PcmScaler::new(bits);
            assert_eq!(scaler.process(&samples, 100), full);
            assert_eq!(scaler.process(&samples, 50), half);
            assert_eq!(scaler.process(&samples, 0), silent);
            assert!(!scaler.has_partial_sample());
        }
    }

    #[test]
    fn handles_samples_split_across_reads() {
        let mut scaler = PcmScaler::new(24);
        assert_eq!(scaler.process(&[0x01, 0x02], 100), []);
        assert!(scaler.has_partial_sample());
        assert_eq!(
            scaler.process(&[0x03, 0x04, 0x05, 0x06, 0x07], 100),
            [0, 1, 2, 3, 0, 4, 5, 6]
        );
        assert!(scaler.has_partial_sample());
        assert_eq!(scaler.process(&[8, 9], 100), [0, 7, 8, 9]);
        assert!(!scaler.has_partial_sample());
    }

    #[test]
    fn handles_odd_mono_sample_count() {
        let mut scaler = PcmScaler::new(16);
        assert_eq!(scaler.process(&[1, 0, 2], 100), []);
        assert!(scaler.has_partial_sample());
        assert_eq!(scaler.process(&[0, 3, 0], 100), [2, 0, 1, 0]);
        assert!(!scaler.has_partial_sample());
        assert_eq!(scaler.finish(100), [0, 0, 3, 0]);
    }
}
