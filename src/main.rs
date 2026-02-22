mod led;
mod sensors;
mod timesource;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{SdCard, VolumeIdx, VolumeManager};
use esp_idf_hal::delay::{FreeRtos};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi;
use esp_idf_hal::gpio::PinDriver;

use crate::timesource::DummyTimesource;


fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let led_pin = peripherals.pins.gpio4;
    let motion_pin = peripherals.pins.gpio5;

    let mut led = led::Led::new(led_pin);
    let motion_sensor = sensors::MotionSensor::new(motion_pin);

    let sclk = peripherals.pins.gpio7;
    let miso = peripherals.pins.gpio6;
    let mosi = peripherals.pins.gpio8;
    let cs = peripherals.pins.gpio9;

    // Create low-level SPI driver (no hardware CS)
    let mut spi_driver = spi::SpiDriver::new(
        peripherals.spi2,
        sclk,
        mosi,
        Some(miso),
        &spi::config::DriverConfig::default(),
    )
    .unwrap();

    // Wrap the driver into a SpiBusDriver (implements embedded-hal SpiBus)
    let spi_bus = spi::SpiBusDriver::new(&mut spi_driver, &spi::config::Config::default()).unwrap();

    // software-controlled CS pin for the SD card
    let sd_cs = PinDriver::output(cs).unwrap();

    let spi_dev = ExclusiveDevice::new(spi_bus, sd_cs, FreeRtos).unwrap();
    let sdcard = SdCard::new(spi_dev, FreeRtos);

    let volume_mgr = VolumeManager::new(sdcard, DummyTimesource());
    let volume0 = volume_mgr.open_volume(VolumeIdx(0)).unwrap();
    log::info!("Volume 0: {:?}", volume0);
    let root_dir = volume0.open_root_dir().unwrap();          
    loop {
        //FreeRtos::delay_ms(100);
        // not detecting falling edges
        // if motion_sensor.take_changed() {
        //     log::info!("motion changed, level={}", motion_sensor.is_moving());
        //     if motion_sensor.is_moving() {
        //         led.light_on();
        //     } else {
        //         led.light_off();
        //     }
        // }

        FreeRtos::delay_ms(1000);
        if motion_sensor.is_moving() {
            led.light_on();
        } else {
            led.light_off();
        }
    }
}
