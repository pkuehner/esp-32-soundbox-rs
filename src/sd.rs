use crate::wav::WaveHeader;
use esp_idf_hal::{
    gpio::{AnyIOPin, InputPin, OutputPin},
    sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver},
    spi::{Dma, SpiDriver, SpiDriverConfig, SPI2},
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

    pub fn playable_files(&mut self) -> std::io::Result<Vec<String>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir("/sdcard")? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if crate::uploads::is_temporary_name(name)
                    || crate::uploads::audio_enabled(name) == Some(false)
                {
                    continue;
                }
                match self.read_wave_file_header(name) {
                    Ok(_) => files.push(name.to_owned()),
                    Err(err) => log::warn!("Skipping {}: {}", name, err),
                }
            }
        }
        Ok(files)
    }

    pub fn read_wave_file_header(&mut self, file_name: &str) -> Result<WaveHeader, String> {
        let path = Self::full_path(file_name);
        let mut file = File::open(path).map_err(|_| "Could not read file".to_owned())?;
        WaveHeader::read(&mut file).map_err(|err| err.to_string())
    }

    /// Stream wav file with configurable ping-pong buffers.
    pub fn stream_wav_file_buf<F>(
        &mut self,
        file_name: &str,
        buf_size: usize,
        mut on_chunk: F,
    ) -> bool
    where
        F: FnMut(&[u8]) -> bool,
    {
        let path = Self::full_path(file_name);
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => return false,
        };

        let header = match WaveHeader::read(&mut file) {
            Ok(header) => header,
            Err(_) => return false,
        };
        let mut file = file.take(u64::from(header.data_len));

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
