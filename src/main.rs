mod led;
mod sensors;
mod sd;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::{
    task::thread::ThreadSpawnConfiguration,
};
use std::thread;

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();
    FreeRtos::delay_ms(1000);

    let peripherals = Peripherals::take().unwrap();
    let led_pin = peripherals.pins.gpio4;
    let motion_pin = peripherals.pins.gpio5;

    let mut led = led::Led::new(led_pin);
    let motion_sensor = sensors::MotionSensor::new(motion_pin);

    let sclk = peripherals.pins.gpio7;
    let miso = peripherals.pins.gpio6;
    let mosi = peripherals.pins.gpio8;
    let cs = peripherals.pins.gpio9;

    // Create low-lev
    let mut sd_fetcher = sd::SdFetcher::new(peripherals.spi2, sclk, mosi, miso, cs);
    log::info!("{}", sd_fetcher.file_exists("test.txt"));
    FreeRtos::delay_ms(1000);

    ThreadSpawnConfiguration {
        stack_size: 4096,
        priority: 10,
        ..Default::default()
    }
    .set()
    .unwrap();

    thread::spawn(move || loop {
        let moving = motion_sensor.is_moving();

        if moving {
            let _ = led.light_on();
        } else {
            let _ = led.light_off();
        }

        FreeRtos::delay_ms(1000);       // non-busy wait
    });

    loop {
        log::info!("Test LOOP");
        FreeRtos::delay_ms(500);       // non-busy wait

    }
}

