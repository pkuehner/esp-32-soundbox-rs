mod led;
mod sensors;
mod sd;
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
    FreeRtos::delay_ms(1000);

    let peripherals = Peripherals::take().unwrap();
    let led_pin = peripherals.pins.gpio2;
    let motion_pin = peripherals.pins.gpio4;

    let mut led = led::Led::new(led_pin);
    let motion_sensor = sensors::MotionSensor::new(motion_pin);

    let sclk = peripherals.pins.gpio18;
    let miso = peripherals.pins.gpio19;
    let mosi = peripherals.pins.gpio23;
    let cs = peripherals.pins.gpio5;

    let file_name = "birds.wav";

    // Create low-lev
    let mut sd_fetcher = sd::SdFetcher::new(peripherals.spi2, sclk, mosi, miso, cs);
    log::info!("{}", sd_fetcher.file_exists(file_name));
    FreeRtos::delay_ms(1000);

    let i2s_bclk = peripherals.pins.gpio26;
    let i2s_dout = peripherals.pins.gpio22;
    let i2s_ws = peripherals.pins.gpio25;
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

    let mut was_moving = false;

    loop {
        let moving = motion_sensor.is_moving();

        if moving && !was_moving {
            let _ = led.light_on();
            let ok = sd_fetcher.stream_file_1024(file_name, |chunk| {
                i2s.write_all(chunk, BLOCK).is_ok()
            });

            if !ok {
                log::warn!("Failed to stream alert.raw to I2S");
            }
        } else {
            let _ = led.light_off();
        }

        was_moving = moving;
        FreeRtos::delay_ms(1000);       // non-busy wait
    }
}

