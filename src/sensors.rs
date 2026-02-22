use esp_idf_hal::gpio::{Input, InputPin, PinDriver};

pub struct MotionSensor<'a, T: InputPin> {
    motion_detected: bool,
    pin_driver: PinDriver<'a, T, Input>,
}

impl<'a, T: InputPin> MotionSensor<'a, T> {
    pub fn new(pin: T) -> Self {
        Self {
            motion_detected: false,
            pin_driver: PinDriver::input(pin).unwrap(),
        }
    }
    pub fn is_moving(&mut self) -> bool {
        self.motion_detected = self.pin_driver.is_high();
        return self.motion_detected;
    }
}
