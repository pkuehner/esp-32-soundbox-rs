use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{Mode, SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use esp_idf_hal::{
    delay::FreeRtos,
    gpio::{InputPin, Output, OutputPin, PinDriver},
    spi::{self, SPI2, SpiDriver},
};

type SdDev<'d, T4> = SdCard<
    ExclusiveDevice<
        spi::SpiBusDriver<'d, SpiDriver<'d>>,
        PinDriver<'d, T4, Output>,
        FreeRtos,
    >,
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

        let spi_bus =
            spi::SpiBusDriver::new(spi_driver, &spi::config::Config::default()).unwrap();

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

    pub fn stream_file_1024<F>(&mut self, file_name: &str, mut on_chunk: F) -> bool
    where
        F: FnMut(&[u8]) -> bool,
    {
        let volume = self.volume_mgr.open_volume(VolumeIdx(0));

        if let Ok(volume) = volume {
            if let Ok(root) = volume.open_root_dir() {
                if let Ok(file) = root.open_file_in_dir(file_name, Mode::ReadOnly) {
                    let mut buffer = [0_u8; 1024];

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