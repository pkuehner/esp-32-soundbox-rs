use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{Mode, SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use esp_idf_hal::{
    delay::FreeRtos,
    gpio::{InputPin, Output, OutputPin, PinDriver},
    spi::{self, SpiDriver, SPI2},
};

type SdDev<'d, T4> = SdCard<
    ExclusiveDevice<spi::SpiBusDriver<'d, SpiDriver<'d>>, PinDriver<'d, T4, Output>, FreeRtos>,
    FreeRtos,
>;

type Vm<'d, T4> = VolumeManager<SdDev<'d, T4>, DummyTimesource, 4, 4, 1>;

pub struct SdFetcher<'d, T4: OutputPin> {
    volume_mgr: Vm<'d, T4>,
}

impl<'d, T4: OutputPin> SdFetcher<'d, T4> {
    pub fn new<T1, T2, T3>(spi2: SPI2, sclk: T1, mosi: T2, miso: T3, cs: T4) -> Self
    where
        T1: OutputPin,
        T2: OutputPin,
        T3: InputPin,
    {
        let spi_driver = spi::SpiDriver::new(
            spi2,
            sclk,
            mosi,
            Some(miso),
            &spi::config::DriverConfig::default(),
        )
        .unwrap();

        let spi_bus = spi::SpiBusDriver::new(spi_driver, &spi::config::Config::default()).unwrap();

        let sd_cs = PinDriver::output(cs).unwrap();
        let spi_dev = ExclusiveDevice::new(spi_bus, sd_cs, FreeRtos).unwrap();
        let sdcard = SdCard::new(spi_dev, FreeRtos);

        let volume_mgr = VolumeManager::new(sdcard, DummyTimesource());
        Self { volume_mgr }
    }

    // Do operations here; avoid returning handles that outlive temporary borrows.
    pub fn file_exists(&mut self, file_name: &str) -> bool {
        let volume = self.volume_mgr.open_volume(VolumeIdx(0));
        if let Ok(volume) = volume {
            if let Ok(root) = volume.open_root_dir() {
                return root.open_file_in_dir(file_name, Mode::ReadOnly).is_ok();
            }
        }
        false
    }

    pub fn read_wave_file_header(&mut self, file_name: &str) -> Result<WaveHeader, String> {
        let volume = self.volume_mgr.open_volume(VolumeIdx(0));

        if let Ok(volume) = volume {
            if let Ok(root) = volume.open_root_dir() {
                if let Ok(file) = root.open_file_in_dir(file_name, Mode::ReadOnly) {
                    if file.length() < 44 {
                        log::info!("Not a wav file");
                        return Err("Not a wav file".to_owned());
                    }

                    let mut buffer_header = [0_u8; 44];
                    file.read(&mut buffer_header).unwrap();

                    return WaveHeader::from_bytes(buffer_header);
                }
            }
        }

        return Err("Could not read file".to_owned());
    }

    pub fn stream_wav_file_1024<F>(&mut self, file_name: &str, mut on_chunk: F) -> bool
    where
        F: FnMut(&[u8]) -> bool,
    {
        let volume = self.volume_mgr.open_volume(VolumeIdx(0));

        if let Ok(volume) = volume {
            if let Ok(root) = volume.open_root_dir() {
                if let Ok(file) = root.open_file_in_dir(file_name, Mode::ReadOnly) {
                    let mut buffer = [0_u8; 1024];
                    if file.length() < 44 {
                        log::info!("Not a wav file");
                        return false;
                    }

                    let mut buffer_header = [0_u8; 44];
                    file.read(&mut buffer_header).unwrap();

                    while !file.is_eof() {
                        match file.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(read_len) => {
                                if !on_chunk(&buffer[..read_len]) {
                                    return false;
                                }
                            }
                            Err(_) => return false,
                        }
                    }

                    return true;
                }
            }
        }

        false
    }
}

/// A dummy timesource, which is mostly important for creating files.
#[derive(Default)]
struct DummyTimesource();

impl TimeSource for DummyTimesource {
    // In theory you could use the RTC of the rp2040 here, if you had
    // any external time synchronizing device.
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

pub struct WaveHeader {
    pub num_channels: u16,    // starts at 22
    pub sample_rate: u32,     // starts at 24
    pub bits_per_sample: u16, // starts at 34
}

impl WaveHeader {
    fn from_bytes(buffer: [u8; 44]) -> Result<Self, String> {
        let num_channels: u16 = u16::from_le_bytes(
            buffer[22..24]
                .try_into()
            .map_err(|_| "Invalid channel data".to_owned())?,
        );
        let sample_rate: u32 = u32::from_le_bytes(
            buffer[24..28]
                .try_into()
            .map_err(|_| "Invalid sample rate data".to_owned())?,
        );
        let bits_per_sample: u16 = u16::from_le_bytes(
            buffer[34..36]
                .try_into()
            .map_err(|_| "Invalid bits per sample data".to_owned())?,
        );
        return Ok(WaveHeader {
            num_channels,
            sample_rate,
            bits_per_sample,
        });
    }
}

#[cfg(test)]
#[allow(dead_code, unused_imports)]
mod tests {
    use super::WaveHeader;

    fn make_header(num_channels: u16, sample_rate: u32, bits_per_sample: u16) -> [u8; 44] {
        let mut header = [0u8; 44];

        header[22..24].copy_from_slice(&num_channels.to_le_bytes());
        header[24..28].copy_from_slice(&sample_rate.to_le_bytes());
        header[34..36].copy_from_slice(&bits_per_sample.to_le_bytes());

        header
    }

    #[test]
    fn parses_16bit_stereo_44k1() {
        let header = make_header(2, 44_100, 16);
        let parsed = WaveHeader::from_bytes(header).unwrap();

        assert_eq!(parsed.num_channels, 2);
        assert_eq!(parsed.sample_rate, 44_100);
        assert_eq!(parsed.bits_per_sample, 16);
    }

    #[test]
    fn parses_24bit_mono_48k() {
        let header = make_header(1, 48_000, 24);
        let parsed = WaveHeader::from_bytes(header).unwrap();

        assert_eq!(parsed.num_channels, 1);
        assert_eq!(parsed.sample_rate, 48_000);
        assert_eq!(parsed.bits_per_sample, 24);
    }
}
