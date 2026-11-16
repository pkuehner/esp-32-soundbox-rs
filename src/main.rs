mod led;
mod rearm;
mod sd;
mod selection;
mod sensors;
mod settings;
mod uploads;
mod volume;
mod wav;
mod web;
use esp_idf_hal::delay::{FreeRtos, BLOCK};
use esp_idf_hal::gpio::AnyIOPin;
use esp_idf_hal::i2s::config::Config as I2sChannelConfig;
use esp_idf_hal::i2s::config::{
    DataBitWidth, MclkMultiple, SlotMode, StdConfig, StdSlotConfig, StdSlotMask,
};
use esp_idf_hal::i2s::{I2sDriver, I2sTx};
use esp_idf_hal::peripherals::Peripherals;
use std::sync::{Arc, Mutex};

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
    let sd_lock = Arc::new(Mutex::new(()));
    let (volume, rearm_delay, _wifi, _server) = match web::start(peripherals.modem, sd_lock.clone())
    {
        Ok(services) => services,
        Err(err) => {
            log::error!("Failed to start web controls: {}", err);
            return;
        }
    };

    let mut i2s_port = peripherals.i2s0;
    let mut i2s_bclk = peripherals.pins.gpio32;
    let mut i2s_dout = peripherals.pins.gpio25;
    let mut i2s_ws = peripherals.pins.gpio13;
    let mut previous_file: Option<String> = None;

    loop {
        if let Some(detected_at) = motion_sensor.take_motion_started() {
            let result = (|| -> Result<(), String> {
                let _sd_guard = sd_lock
                    .lock()
                    .map_err(|err| format!("SD lock failed: {err}"))?;
                let files = sd_fetcher.playable_files().map_err(|err| err.to_string())?;
                let file_name = selection::select(&files, previous_file.as_deref(), || unsafe {
                    esp_idf_sys::esp_random()
                })
                .ok_or_else(|| "No supported WAV files on SD card".to_owned())?;
                let header = sd_fetcher.read_wave_file_header(file_name)?;
                let bits = match header.bits_per_sample {
                    8 => DataBitWidth::Bits8,
                    16 => DataBitWidth::Bits16,
                    24 => DataBitWidth::Bits24,
                    32 => DataBitWidth::Bits32,
                    _ => return Err("Unsupported sample width".to_owned()),
                };
                let mut clock =
                    esp_idf_hal::i2s::config::StdClkConfig::from_sample_rate_hz(header.sample_rate);
                if header.bits_per_sample == 24 {
                    clock = clock.mclk_multiple(MclkMultiple::M384);
                }
                let config = StdConfig::new(
                    I2sChannelConfig::default(),
                    clock,
                    StdSlotConfig::philips_slot_default(bits, SlotMode::Mono)
                        .slot_mode_mask(SlotMode::Mono, StdSlotMask::Left),
                    Default::default(),
                );
                let mut i2s = I2sDriver::<I2sTx>::new_std_tx(
                    &mut i2s_port,
                    &config,
                    &mut i2s_bclk,
                    &mut i2s_dout,
                    AnyIOPin::none(),
                    &mut i2s_ws,
                )
                .map_err(|err| err.to_string())?;
                i2s.tx_enable().map_err(|err| err.to_string())?;
                log::info!(
                    "Playing {} ({} Hz, {}-bit, {} channel(s))",
                    file_name,
                    header.sample_rate,
                    header.bits_per_sample,
                    header.num_channels
                );
                let mut scaler = volume::PcmScaler::new(header.bits_per_sample);
                let ok = sd_fetcher.stream_wav_file_buf(file_name, 4096, |chunk| {
                    let scaled = scaler.process(chunk, volume.load());
                    i2s.write_all(scaled, BLOCK).is_ok()
                });
                let finished = if ok && !scaler.has_partial_sample() {
                    i2s.write_all(scaler.finish(volume.load()), BLOCK).is_ok()
                } else {
                    false
                };
                i2s.tx_disable().map_err(|err| err.to_string())?;
                if !ok {
                    return Err(format!("Failed to stream {}", file_name));
                }
                if scaler.has_partial_sample() {
                    return Err(format!("Incomplete PCM sample in {}", file_name));
                }
                if !finished {
                    return Err(format!("Failed to finish streaming {}", file_name));
                }
                previous_file = Some(file_name.to_owned());
                Ok(())
            })();
            if let Err(err) = result {
                log::warn!("Playback failed: {}", err);
            }
            loop {
                let remaining =
                    rearm::remaining_ms(detected_at, sensors::now_ms(), rearm_delay.load());
                if remaining == 0 {
                    break;
                }
                FreeRtos::delay_ms(remaining.min(100));
            }
            if let Err(err) = motion_sensor.rearm_interrupt() {
                log::warn!("Failed to re-arm motion interrupt: {:?}", err);
            } else {
                log::info!("Motion Interrupt re-armed!");
            }
        }
        FreeRtos::delay_ms(500); // non-busy wait
    }
}
