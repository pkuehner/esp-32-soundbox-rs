use esp_idf_hal::{
    delay::FreeRtos,
    gpio::{AnyIOPin, InputPin, OutputPin},
    sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver},
    spi::{Dma, SPI2, SpiDriver, SpiDriverConfig},
    task,
};
use esp_idf_svc::fs::fatfs::Fatfs;
use esp_idf_svc::io::vfs::MountedFatfs;
use esp_idf_svc::sys::EspError;
use std::fs::File;
use std::io::Read;

const FREERTOS_STREAM_CHUNK_BYTES: usize = 4096;

#[repr(C)]
#[derive(Clone, Copy)]
struct StreamChunk {
    len: u32,
    done: u8,
    ok: u8,
    _reserved: [u8; 2],
    data: [u8; FREERTOS_STREAM_CHUNK_BYTES],
}

impl StreamChunk {
    fn new_data(chunk: &[u8]) -> Self {
        let mut message = Self {
            len: chunk.len() as u32,
            done: 0,
            ok: 1,
            _reserved: [0; 2],
            data: [0; FREERTOS_STREAM_CHUNK_BYTES],
        };

        message.data[..chunk.len()].copy_from_slice(chunk);
        message
    }

    fn new_done(ok: bool) -> Self {
        Self {
            len: 0,
            done: 1,
            ok: if ok { 1 } else { 0 },
            _reserved: [0; 2],
            data: [0; FREERTOS_STREAM_CHUNK_BYTES],
        }
    }
}

struct ReaderTaskCtx {
    fetcher: *mut core::ffi::c_void,
    queue: esp_idf_sys::QueueHandle_t,
    file_name: String,
}

extern "C" fn reader_task_entry(arg: *mut core::ffi::c_void) {
    let ctx = unsafe { Box::from_raw(arg as *mut ReaderTaskCtx) };

    let fetcher = unsafe { &mut *(ctx.fetcher as *mut SdFetcher<'static>) };

    let send_chunk = |message: &StreamChunk| -> bool {
        if ctx.queue.is_null() {
            return false;
        }

        let max_retries = 10_000u32;

        for _ in 0..max_retries {
            let sent = unsafe {
                esp_idf_sys::xQueueGenericSend(
                    ctx.queue,
                    message as *const _ as *const core::ffi::c_void,
                    0,
                    0,
                )
            };

            if sent != 0 {
                return true;
            }

            FreeRtos::delay_ms(1);
        }

        false
    };

    let ok = fetcher.stream_wav_file_buf(&ctx.file_name, FREERTOS_STREAM_CHUNK_BYTES, |chunk| {
        if chunk.len() > FREERTOS_STREAM_CHUNK_BYTES {
            return false;
        }

        let message = StreamChunk::new_data(chunk);
        send_chunk(&message)
    });

    let done = StreamChunk::new_done(ok);
    let _ = send_chunk(&done);

    unsafe { esp_idf_sys::vTaskDelete(core::ptr::null_mut()) };
}

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

    pub fn stream_wav_file_freertos<F>(&mut self, file_name: &str, mut on_chunk: F) -> bool
    where
        F: FnMut(&[u8]) -> bool,
    {
        let queue = unsafe {
            esp_idf_sys::xQueueGenericCreate(
                2,
                core::mem::size_of::<StreamChunk>() as esp_idf_sys::UBaseType_t,
                0,
            )
        };

        if queue.is_null() {
            log::error!("Failed to create FreeRTOS queue for WAV streaming");
            return false;
        }

        let task_ctx = Box::new(ReaderTaskCtx {
            fetcher: self as *mut _ as *mut core::ffi::c_void,
            queue,
            file_name: file_name.to_owned(),
        });

        let task_name = core::ffi::CStr::from_bytes_with_nul(b"wav_reader\0").unwrap();

        let created = unsafe {
            task::create(
                reader_task_entry,
                task_name,
                16384,
                Box::into_raw(task_ctx) as *mut core::ffi::c_void,
                5,
                None,
            )
        };

        if created.is_err() {
            unsafe { esp_idf_sys::vQueueDelete(queue) };
            log::error!("Failed to create FreeRTOS reader task");
            return false;
        }

        let mut overall_ok = true;

        loop {
            let mut message = StreamChunk::new_done(false);
            let received = unsafe {
                esp_idf_sys::xQueueReceive(
                    queue,
                    &mut message as *mut _ as *mut core::ffi::c_void,
                    0,
                )
            };

            if received == 0 {
                FreeRtos::delay_ms(1);
                continue;
            }

            if message.done != 0 {
                overall_ok = overall_ok && message.ok != 0;
                break;
            }

            if !on_chunk(&message.data[..message.len as usize]) {
                overall_ok = false;
            }
        }

        // Give producer task a moment to exit after sending done marker.
        FreeRtos::delay_ms(10);
        unsafe { esp_idf_sys::vQueueDelete(queue) };
        overall_ok
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
