mod led;
mod sd;
mod sensors;
use esp_idf_hal::delay::{FreeRtos, BLOCK};
use esp_idf_hal::gpio::AnyIOPin;
use esp_idf_hal::i2s::config::{DataBitWidth, StdConfig};
use esp_idf_hal::i2s::{I2sDriver, I2sTx};
use esp_idf_hal::peripherals::Peripherals;

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let motion_pin = peripherals.pins.gpio22;
    log::info!("Creating Motion Sensor");

    let mut motion_sensor = sensors::MotionSensor::new(motion_pin);

    log::info!("Motion Sensor Created");

    let sclk = peripherals.pins.gpio33;
    let miso = peripherals.pins.gpio14;
    let mosi = peripherals.pins.gpio26;
    let cs = peripherals.pins.gpio27;

    let file_name = "birds.wav";

    log::info!("Creating SD");

    // Create low-lev
    let mut sd_fetcher = sd::SdFetcher::new(peripherals.spi2, sclk, mosi, miso, cs);
    
    log::info!("SD Created");

    log::info!("Checking if file: {} exists:  {}", file_name, sd_fetcher.file_exists(file_name));


    let i2s_bclk = peripherals.pins.gpio32;
    let i2s_dout = peripherals.pins.gpio25;
    let i2s_ws = peripherals.pins.gpio13;
    let i2s_config = StdConfig::philips(44100, DataBitWidth::Bits16);
    let mut i2s = I2sDriver::<I2sTx>::new_std_tx(
        peripherals.i2s0,
        &i2s_config,
        i2s_bclk,
        i2s_dout,
        AnyIOPin::none(),
        i2s_ws,
    )
    .unwrap();
    i2s.tx_enable().unwrap();

    loop {
        if motion_sensor.take_motion_started() {
            if let Err(err) = motion_sensor.rearm_interrupt() {
                log::warn!("Failed to re-arm motion interrupt: {:?}", err);
            }

            log::info!("Motion Started");
            let ok = sd_fetcher
                .stream_file_1024(file_name, |chunk| i2s.write_all(chunk, BLOCK).is_ok());

            if !ok {
                log::warn!("Failed to stream alert.raw to I2S");
            }
        }
        FreeRtos::delay_ms(500); // non-busy wait
    }
}
