use esp_idf_hal::{
    gpio::{AnyIOPin, InputPin, OutputPin},
    sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver},
    spi::{Dma, SPI2, SpiDriver, SpiDriverConfig},
};
use esp_idf_svc::fs::fatfs::Fatfs;
use esp_idf_svc::io::vfs::MountedFatfs;
use esp_idf_svc::sys::EspError;
use std::fs::File;
use std::io::Read;

type MountedSd<'d> = MountedFatfs<Fatfs<SdCardDriver<SdSpiHostDriver<'d, SpiDriver<'d>>>>>;

pub struct SdFetcher<'d> {
    _mounted: MountedSd<'d>,
}

impl<'d> SdFetcher<'d> {
    pub fn new<T1, T2, T3, T4>(
        spi2: SPI2,
        sclk: T1,
        mosi: T2,
        miso: T3,
        cs: T4,
    ) -> Result<Self, EspError>
    where
        T1: OutputPin,
        T2: OutputPin,
        T3: InputPin,
        T4: OutputPin,
    {
        let spi_driver = SpiDriver::new(
            spi2,
            sclk,
            mosi,
            Some(miso),
            &SpiDriverConfig::new().dma(Dma::Auto(8192)),
        )?;

        let sd_host_driver = SdSpiHostDriver::new(
            spi_driver,
            Some(cs),
            AnyIOPin::none(),
            AnyIOPin::none(),
            AnyIOPin::none(),
            None,
        )?;

        let mut sd_cfg = SdCardConfiguration::new();
        sd_cfg.speed_khz = 4_000;
        sd_cfg.command_timeout_ms = 2_000;

        let card_driver = SdCardDriver::new_spi(sd_host_driver, &sd_cfg)?;
        let fatfs = Fatfs::new_sdcard(0, card_driver)?;
        let mounted = MountedFatfs::mount(fatfs, "/sdcard", 5)?;

        Ok(Self { _mounted: mounted })
    }

    fn full_path(file_name: &str) -> String {
        format!("/sdcard/{file_name}")
    }

    // Do operations here; avoid returning handles that outlive temporary borrows.
    pub fn file_exists(&mut self, file_name: &str) -> bool {
        std::fs::metadata(Self::full_path(file_name)).is_ok()
    }

    pub fn read_wave_file_header(&mut self, file_name: &str) -> Result<WaveHeader, String> {
        let path = Self::full_path(file_name);
        let mut file = File::open(path).map_err(|_| "Could not read file".to_owned())?;

        let mut buffer_header = [0_u8; 44];
        file.read_exact(&mut buffer_header)
            .map_err(|_| "Not a wav file".to_owned())?;

        WaveHeader::from_bytes(buffer_header)
    }


    /// Stream wav file with configurable ping-pong buffers.
    pub fn stream_wav_file_buf<F>(&mut self, file_name: &str, buf_size: usize, mut on_chunk: F) -> bool
    where
        F: FnMut(&[u8]) -> bool,
    {
        let path = Self::full_path(file_name);
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => return false,
        };

        let mut buffer_header = [0_u8; 44];
        if file.read_exact(&mut buffer_header).is_err() {
            log::info!("Not a wav file");
            return false;
        }

        let mut buffer_a = vec![0_u8; buf_size];
        let mut buffer_b = vec![0_u8; buf_size];

        let mut current_len = match file.read(&mut buffer_a) {
            Ok(0) => return true,
            Ok(read_len) => read_len,
            Err(_) => return false,
        };

        loop {
            if !on_chunk(&buffer_a[..current_len]) {
                return false;
            }

            let next_len = match file.read(&mut buffer_b) {
                Ok(0) => break,
                Ok(read_len) => read_len,
                Err(_) => return false,
            };

            core::mem::swap(&mut buffer_a, &mut buffer_b);
            current_len = next_len;
        }

        true
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
