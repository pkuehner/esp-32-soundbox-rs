use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub const MAX_SECONDS: u32 = 4_000_000;

#[derive(Clone)]
pub struct RearmDelay(Arc<AtomicU32>);

impl RearmDelay {
    pub fn new(seconds: u32) -> Self {
        assert!(seconds <= MAX_SECONDS);
        Self(Arc::new(AtomicU32::new(seconds)))
    }

    pub fn load(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn store(&self, seconds: u32) {
        assert!(seconds <= MAX_SECONDS);
        self.0.store(seconds, Ordering::Relaxed);
    }
}

pub fn remaining_ms(detected_at: u32, now: u32, delay_seconds: u32) -> u32 {
    (delay_seconds * 1_000).saturating_sub(now.wrapping_sub(detected_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldown_counts_from_detection_and_never_extends_past_zero() {
        assert_eq!(remaining_ms(1_000, 1_000, 10), 10_000);
        assert_eq!(remaining_ms(1_000, 5_000, 10), 6_000);
        assert_eq!(remaining_ms(1_000, 12_000, 10), 0);
        assert_eq!(remaining_ms(1_000, 1_000, 0), 0);
        assert_eq!(remaining_ms(1_000, 2_000, MAX_SECONDS), 3_999_999_000);
    }

    #[test]
    fn cooldown_handles_clock_wrap_and_setting_changes() {
        assert_eq!(remaining_ms(u32::MAX - 499, 500, 2), 1_000);
        let delay = RearmDelay::new(10);
        assert_eq!(remaining_ms(0, 5_000, delay.load()), 5_000);
        delay.store(2);
        assert_eq!(remaining_ms(0, 5_000, delay.load()), 0);
        delay.store(20);
        assert_eq!(remaining_ms(0, 5_000, delay.load()), 15_000);
    }
}
