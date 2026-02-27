mod led;
mod sd;
mod sensors;
use esp_idf_hal::delay::{FreeRtos, BLOCK};
use esp_idf_hal::gpio::AnyIOPin;
use esp_idf_hal::i2s::config::{DataBitWidth, SlotMode, StdConfig, StdSlotConfig, StdSlotMask};
use esp_idf_hal::i2s::{I2sDriver, I2sTx};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::i2s::config::Config as I2sChannelConfig;


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
    let mut sd_fetcher = match sd::SdFetcher::new(peripherals.spi2, sclk, mosi, miso, cs) {
        Ok(sd) => sd,
        Err(err) => {
            log::error!(
                "Failed to initialize SD card over SPI (check wiring and lower clock). Error: {}",
                err
            );
            return;
        }
    };
    
    log::info!("SD Created");

    log::info!("Checking if file: {} exists:  {}", file_name, sd_fetcher.file_exists(file_name));


    let i2s_bclk = peripherals.pins.gpio32;
    let i2s_dout = peripherals.pins.gpio25;
    let i2s_ws = peripherals.pins.gpio13;
    let header = match sd_fetcher.read_wave_file_header(file_name) {
        Ok(header) => header,
        Err(err) => {
            log::error!("Failed to read WAV header for {}: {}", file_name, err);
            return;
        }
    };

    let bits_per_sample = match header.bits_per_sample {
        8 => DataBitWidth::Bits8,
        16 => DataBitWidth::Bits16,
        24 => DataBitWidth::Bits24,
        32 => DataBitWidth::Bits32,
        other => {
            log::error!("Unsupported bits per sample in WAV header: {}", other);
            return;
        }
    };

    if header.num_channels != 1 {
        log::error!("Invalid channel count in WAV header! We only support Mono");
        return;
    }

    log::info!(
        "Configuring I2S from WAV header: {} Hz, {}-bit, {} channel(s)",
        header.sample_rate,
        header.bits_per_sample,
        header.num_channels
    );

    let channel_cfg = I2sChannelConfig::default();

    let i2s_config = StdConfig::new(
        channel_cfg,
        esp_idf_hal::i2s::config::StdClkConfig::from_sample_rate_hz(header.sample_rate),
        StdSlotConfig::philips_slot_default(bits_per_sample, SlotMode::Mono)
            .slot_mode_mask(SlotMode::Mono, StdSlotMask::Left),
        Default::default(),
    );
    let mut i2s = I2sDriver::<I2sTx>::new_std_tx(
        peripherals.i2s0,
        &i2s_config,
        i2s_bclk,
        i2s_dout,
        AnyIOPin::none(),
        i2s_ws,
    )
    .unwrap();
    let mut i2s_enabled = false;

    loop {
        if motion_sensor.take_motion_started() {
            if let Err(err) = motion_sensor.rearm_interrupt() {
                log::warn!("Failed to re-arm motion interrupt: {:?}", err);
            }

            if !i2s_enabled {
                if let Err(err) = i2s.tx_enable() {
                    log::error!("Failed to enable I2S TX: {:?}", err);
                    FreeRtos::delay_ms(500);
                    continue;
                }
                i2s_enabled = true;
            }

            log::info!("Motion Started");

            let ok = sd_fetcher.stream_wav_file_freertos(file_name, |chunk| {
                let res = i2s.write_all(chunk, BLOCK).is_ok();
                res
            });

            if !ok {
                log::warn!("Failed to stream alert.raw to I2S");
            }
        } else if i2s_enabled {
            if let Err(err) = i2s.tx_disable() {
                log::warn!("Failed to disable I2S TX: {:?}", err);
            } else {
                i2s_enabled = false;
            }
        }
        FreeRtos::delay_ms(500); // non-busy wait
    }
}
