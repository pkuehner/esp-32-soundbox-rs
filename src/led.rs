use esp_idf_hal::gpio::{Output, OutputPin, PinDriver};

pub struct Led<'a, T: OutputPin> {
    pin_driver: PinDriver<'a, T, Output>,
}

impl<'a, T: OutputPin> Led<'a, T> {
    pub fn new(pin: T) -> Self {
        Self {
            pin_driver: PinDriver::output(pin).unwrap(),
        }
    }
    pub fn light_on(&mut self) {
        self.pin_driver.set_high().unwrap();
    }

    pub fn light_off(&mut self) {
        self.pin_driver.set_low().unwrap();
    }
}
