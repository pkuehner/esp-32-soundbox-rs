use std::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use esp_idf_hal::gpio::{Input, InputPin, InterruptType, OutputPin, PinDriver, Pull};
use esp_idf_sys::EspError;

pub struct MotionSensor<'a, T: InputPin + OutputPin> {
    pin_driver: PinDriver<'a, T, Input>,
    changed: Arc<AtomicBool>,
}

impl<'a, T: InputPin + OutputPin> MotionSensor<'a, T> {
    pub fn new(pin: T) -> Self {
        let mut sensor = Self {
            pin_driver: PinDriver::input(pin).unwrap(),
            changed: Arc::new(AtomicBool::new(false)),
        };
        
        sensor.pin_driver.set_pull(Pull::Up).unwrap();
        sensor.enable_change_interrupt().unwrap();

        sensor
    }

    pub fn is_moving(&self) -> bool {
        self.pin_driver.is_high()
    }

    pub fn enable_change_interrupt(&mut self) -> Result<(), EspError> {
        self.pin_driver.set_interrupt_type(InterruptType::AnyEdge)?;

        let changed = Arc::clone(&self.changed);

        unsafe {
            self.pin_driver.subscribe(move || {
                changed.store(true, Ordering::SeqCst);
            })?;
        }

        self.pin_driver.enable_interrupt()?;
        Ok(())
    }

    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::SeqCst)
    }
}
