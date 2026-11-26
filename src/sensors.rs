use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::gpio::{Input, InputPin, InterruptType, OutputPin, PinDriver, Pull};
use esp_idf_sys::EspError;
use std::sync::Arc;

/// Configuration for sustained-HIGH motion qualification
pub struct MotionDetectionConfig {
    /// How long the pin must remain continuously HIGH to confirm real motion (ms)
    pub confirm_ms: u32,
    /// Polling interval while verifying the pin stays HIGH (ms)
    pub sample_interval_ms: u32,
}

impl Default for MotionDetectionConfig {
    fn default() -> Self {
        Self {
            confirm_ms: 200,
            sample_interval_ms: 25,
        }
    }
}

pub struct MotionSensor<'a, T: InputPin + OutputPin> {
    pin_driver: PinDriver<'a, T, Input>,
    motion_started: Arc<AtomicBool>,
    detected_at: Arc<AtomicU32>,
    config: MotionDetectionConfig,
}

impl<'a, T: InputPin + OutputPin> MotionSensor<'a, T> {
    pub fn new(pin: T) -> Self {
        Self::with_config(pin, MotionDetectionConfig::default())
    }

    pub fn with_config(pin: T, config: MotionDetectionConfig) -> Self {
        let mut sensor = Self {
            pin_driver: PinDriver::input(pin).unwrap(),
            motion_started: Arc::new(AtomicBool::new(false)),
            detected_at: Arc::new(AtomicU32::new(0)),
            config,
        };

        sensor.pin_driver.set_pull(Pull::Down).unwrap();
        sensor.enable_change_interrupt().unwrap();

        sensor
    }

    pub fn _is_moving(&self) -> bool {
        self.pin_driver.is_high()
    }

    pub fn enable_change_interrupt(&mut self) -> Result<(), EspError> {
        self.pin_driver.set_interrupt_type(InterruptType::PosEdge)?;

        let motion_started = Arc::clone(&self.motion_started);
        let detected_at = Arc::clone(&self.detected_at);

        unsafe {
            self.pin_driver.subscribe(move || {
                detected_at.store(now_ms(), Ordering::SeqCst);
                motion_started.store(true, Ordering::SeqCst);
            })?;
        }

        self.pin_driver.enable_interrupt()?;
        Ok(())
    }

    pub fn take_motion_started(&mut self) -> Option<u32> {
        if !self.motion_started.swap(false, Ordering::SeqCst) {
            return None;
        }

        let detected_at = self.detected_at.load(Ordering::SeqCst);
        if self.is_sustained_high() {
            Some(detected_at)
        } else {
            log::debug!("Ignored short motion glitch; re-arming interrupt");
            if let Err(err) = self.rearm_interrupt() {
                log::warn!("Failed to re-arm motion interrupt after glitch: {:?}", err);
            }
            None
        }
    }

    fn is_sustained_high(&self) -> bool {
        let start = now_ms();
        loop {
            if !self.pin_driver.is_high() {
                return false;
            }
            let elapsed = now_ms().wrapping_sub(start);
            if elapsed >= self.config.confirm_ms {
                return true;
            }
            let remaining = self.config.confirm_ms - elapsed;
            FreeRtos::delay_ms(self.config.sample_interval_ms.min(remaining).max(1));
        }
    }

    pub fn rearm_interrupt(&mut self) -> Result<(), EspError> {
        self.pin_driver.enable_interrupt()
    }
}

pub fn now_ms() -> u32 {
    (unsafe { esp_idf_sys::esp_timer_get_time() } / 1_000) as u32
}

