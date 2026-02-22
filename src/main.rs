mod sensors;
mod led;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let led_pin = peripherals.pins.gpio4;
    let motion_pin = peripherals.pins.gpio10;

    let mut led = led::Led::new(led_pin);
    let mut motion_sensor = sensors::MotionSensor::new(motion_pin);



    loop {
        FreeRtos::delay_ms(1000);
        log::info!("Hello, world!");
        if motion_sensor.is_moving() {
            led.light_on();
        }
        else{
            led.light_off();
        }
        log::info!("Hello, world!");

    }
}
