use std::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use esp_idf_hal::gpio::{Input, InputPin, InterruptType, OutputPin, PinDriver, Pull};
use esp_idf_sys::EspError;

pub struct MotionSensor<'a, T: InputPin + OutputPin> {
    pin_driver: PinDriver<'a, T, Input>,
    motion_started: Arc<AtomicBool>,
}

impl<'a, T: InputPin + OutputPin> MotionSensor<'a, T> {
    pub fn new(pin: T) -> Self {
        let mut sensor = Self {
            pin_driver: PinDriver::input(pin).unwrap(),
            motion_started: Arc::new(AtomicBool::new(false)),
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

        unsafe {
            self.pin_driver.subscribe(move || {
                motion_started.store(true, Ordering::SeqCst);
            })?;
        }

        self.pin_driver.enable_interrupt()?;
        Ok(())
    }

    pub fn take_motion_started(&self) -> bool {
        self.motion_started.swap(false, Ordering::SeqCst)
    }

    pub fn rearm_interrupt(&mut self) -> Result<(), EspError> {
        self.pin_driver.enable_interrupt()
    }
}
