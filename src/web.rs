use crate::rearm::{RearmDelay, MAX_SECONDS};
use crate::volume::Volume;
use esp_idf_hal::modem::Modem;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::http::{server::EspHttpServer, Method};
use esp_idf_svc::io::{EspIOError, Write};
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, Configuration, EspWifi,
};
use std::path::Path;
use std::sync::{Arc, Mutex};

const SSID: &str = "Jawois Soundhäusle";
const PASSWORD: &str = env!("SOUNDBOX_WIFI_PASSWORD");
const PAGE: &str = include_str!("volume.html");

pub fn start(
    modem: Modem,
    sd_lock: Arc<Mutex<()>>,
) -> Result<
    (
        Volume,
        RearmDelay,
        BlockingWifi<EspWifi<'static>>,
        EspHttpServer<'static>,
    ),
    Box<dyn std::error::Error>,
> {
    let event_loop = EspSystemEventLoop::take()?;
    let partition = EspDefaultNvsPartition::take()?;
    let nvs = EspNvs::new(partition.clone(), "soundbox", true)?;
    let rearm_nvs = EspNvs::new(partition.clone(), "soundbox", true)?;
    let saved = nvs.get_u8("volume")?.unwrap_or(10);
    if saved > 100 {
        return Err(format!("Invalid saved volume: {saved}").into());
    }
    let volume = Volume::new(saved);
    let saved_rearm = rearm_nvs.get_u32("rearm_sec")?.unwrap_or(60);
    if saved_rearm > MAX_SECONDS {
        return Err(format!("Invalid saved rearm delay: {saved_rearm}").into());
    }
    let rearm_delay = RearmDelay::new(saved_rearm);
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, event_loop.clone(), Some(partition))?,
        event_loop,
    )?;
    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: SSID.try_into().map_err(|_| "Invalid Wi-Fi SSID")?,
        password: PASSWORD.try_into().map_err(|_| "Invalid Wi-Fi password")?,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.wait_netif_up()?;
    log::info!(
        "Wi-Fi {SSID} ready at http://{}",
        wifi.wifi().ap_netif().get_ip_info()?.ip
    );

    let mut server = EspHttpServer::new(&Default::default())?;
    server.fn_handler::<EspIOError, _>("/", Method::Get, |request| {
        request
            .into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Cache-Control", "no-store"),
                ],
            )?
            .write_all(PAGE.as_bytes())
    })?;
    let get_volume = volume.clone();
    let set_volume = volume.clone();
    register_setting(
        &mut server,
        "/volume",
        100,
        move || u32::from(get_volume.load()),
        move |value| {
            nvs.set_u8("volume", value as u8)?;
            set_volume.store(value as u8);
            Ok(())
        },
    )?;
    let get_rearm = rearm_delay.clone();
    let set_rearm = rearm_delay.clone();
    register_setting(
        &mut server,
        "/rearm",
        MAX_SECONDS,
        move || get_rearm.load(),
        move |value| {
            rearm_nvs.set_u32("rearm_sec", value)?;
            set_rearm.store(value);
            Ok(())
        },
    )?;
    register_files(&mut server, sd_lock)?;
    Ok((volume, rearm_delay, wifi, server))
}

fn register_files(
    server: &mut EspHttpServer<'static>,
    sd_lock: Arc<Mutex<()>>,
) -> Result<(), esp_idf_svc::sys::EspError> {
    let list_lock = sd_lock.clone();
    server.fn_handler::<EspIOError, _>("/files", Method::Get, move |request| {
        let _guard = match list_lock.try_lock() {
            Ok(guard) => guard,
            Err(err) => {
                log::warn!("SD card unavailable for listing: {err}");
                return request
                    .into_status_response(503)?
                    .write_all("SD-Karte ist belegt.".as_bytes());
            }
        };
        let names = crate::uploads::list_files(Path::new("/sdcard"));
        let names = match names {
            Ok(names) => names,
            Err(err) => {
                log::error!("Failed to list SD card: {err}");
                return request
                    .into_status_response(500)?
                    .write_all("Dateien konnten nicht aufgelistet werden.".as_bytes());
            }
        };
        let json = format!(
            "[{}]",
            names
                .iter()
                .map(|name| crate::uploads::json_string(name))
                .collect::<Vec<_>>()
                .join(",")
        );
        request
            .into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-store"),
                ],
            )?
            .write_all(json.as_bytes())
    })?;
    let upload_lock = sd_lock.clone();
    server.fn_handler::<EspIOError, _>("/files", Method::Post, move |mut request| {
        let name = request.header("X-Filename").map(str::to_owned);
        let length = request
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok());
        if request.header("Content-Type") != Some("application/octet-stream")
            || !name.as_deref().is_some_and(crate::uploads::valid_name)
            || !matches!(length, Some(1..=4_294_967_295))
        {
            return request
                .into_status_response(400)?
                .write_all("Ungültiger Dateiname, Dateityp oder Dateigröße.".as_bytes());
        }
        let _guard = match upload_lock.try_lock() {
            Ok(guard) => guard,
            Err(err) => {
                log::warn!("SD card unavailable for upload: {err}");
                return request.into_status_response(503)?.write_all(
                    "SD-Karte ist belegt. Versuche es nach der Wiedergabe erneut.".as_bytes(),
                );
            }
        };
        let result = crate::uploads::receive(
            Path::new("/sdcard"),
            name.as_deref().unwrap(),
            length.unwrap(),
            |buf| {
                request
                    .read(buf)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
            },
            || unsafe { esp_idf_sys::esp_random() },
        );
        match result {
            Ok(()) => request.into_status_response(201)?.write_all(b"Uploaded"),
            Err(err) => {
                log::warn!("Upload failed: {err}");
                let (status, message) = match err.kind() {
                    std::io::ErrorKind::InvalidInput => {
                        (400, "Ungültiger Dateiname oder ungültige Dateigröße.")
                    }
                    std::io::ErrorKind::InvalidData => {
                        (400, "Ungültige oder unvollständige Mono-PCM-WAV-Datei.")
                    }
                    std::io::ErrorKind::UnexpectedEof => (400, "Hochladen wurde unterbrochen."),
                    std::io::ErrorKind::AlreadyExists => {
                        (409, "Datei oder temporäre Datei existiert bereits.")
                    }
                    _ => (500, "Datei konnte nicht gespeichert werden."),
                };
                request
                    .into_status_response(status)?
                    .write_all(message.as_bytes())
            }
        }
    })?;
    let toggle_lock = sd_lock.clone();
    server.fn_handler::<EspIOError, _>("/files/toggle", Method::Post, move |mut request| {
        let length = request
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok());
        if request.header("Content-Type") != Some("application/octet-stream")
            || !matches!(length, Some(1..=255))
        {
            return request
                .into_status_response(400)?
                .write_all("Ungültige Anfrage.".as_bytes());
        }
        let mut bytes = vec![0; length.unwrap()];
        let mut offset = 0;
        while offset < bytes.len() {
            let count = request.read(&mut bytes[offset..])?;
            if count == 0 {
                return request
                    .into_status_response(400)?
                    .write_all("Unvollständige Anfrage.".as_bytes());
            }
            offset += count;
        }
        let Some(name) = std::str::from_utf8(&bytes).ok() else {
            return request
                .into_status_response(400)?
                .write_all("Ungültiger Dateiname.".as_bytes());
        };
        let _guard = match toggle_lock.try_lock() {
            Ok(guard) => guard,
            Err(err) => {
                log::warn!("SD card unavailable for toggling: {err}");
                return request.into_status_response(503)?.write_all(
                    "SD-Karte ist belegt. Versuche es nach der Wiedergabe erneut.".as_bytes(),
                );
            }
        };
        match crate::uploads::toggle_audio(Path::new("/sdcard"), name) {
            Ok(()) => request.into_ok_response()?.write_all(b"OK"),
            Err(err) => {
                log::warn!("Audio file toggle failed: {err}");
                let (status, message) = match err.kind() {
                    std::io::ErrorKind::InvalidInput => (400, "Ungültiger Dateiname."),
                    std::io::ErrorKind::NotFound => (404, "Datei nicht gefunden."),
                    std::io::ErrorKind::AlreadyExists => {
                        (409, "Eine Datei mit dem Zielnamen existiert bereits.")
                    }
                    _ => (500, "Datei konnte nicht umbenannt werden."),
                };
                request
                    .into_status_response(status)?
                    .write_all(message.as_bytes())
            }
        }
    })?;
    server.fn_handler::<EspIOError, _>("/files", Method::Delete, move |mut request| {
        let length = request
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok());
        if request.header("Content-Type") != Some("application/octet-stream")
            || !matches!(length, Some(1..=255))
        {
            return request
                .into_status_response(400)?
                .write_all("Ungültige Anfrage.".as_bytes());
        }
        let mut bytes = vec![0; length.unwrap()];
        let mut offset = 0;
        while offset < bytes.len() {
            let count = request.read(&mut bytes[offset..])?;
            if count == 0 {
                return request
                    .into_status_response(400)?
                    .write_all("Unvollständige Anfrage.".as_bytes());
            }
            offset += count;
        }
        let Some(name) = std::str::from_utf8(&bytes)
            .ok()
            .filter(|name| crate::uploads::valid_existing_name(name))
        else {
            return request
                .into_status_response(400)?
                .write_all("Ungültiger Dateiname.".as_bytes());
        };
        let _guard = match sd_lock.try_lock() {
            Ok(guard) => guard,
            Err(err) => {
                log::warn!("SD card unavailable for deletion: {err}");
                return request.into_status_response(503)?.write_all(
                    "SD-Karte ist belegt. Versuche es nach der Wiedergabe erneut.".as_bytes(),
                );
            }
        };
        match crate::uploads::delete(Path::new("/sdcard"), name) {
            Ok(()) => request.into_ok_response()?.write_all(b"Deleted"),
            Err(err) => {
                log::warn!("File deletion failed: {err}");
                let status = match err.kind() {
                    std::io::ErrorKind::InvalidInput => 400,
                    std::io::ErrorKind::NotFound => 404,
                    _ => 500,
                };
                request
                    .into_status_response(status)?
                    .write_all("Datei konnte nicht gelöscht werden.".as_bytes())
            }
        }
    })?;
    Ok(())
}

fn register_setting(
    server: &mut EspHttpServer<'static>,
    path: &str,
    max: u32,
    get: impl Fn() -> u32 + Send + Clone + 'static,
    set: impl Fn(u32) -> Result<(), esp_idf_svc::sys::EspError> + Send + 'static,
) -> Result<(), esp_idf_svc::sys::EspError> {
    let get_current = get.clone();
    server.fn_handler::<EspIOError, _>(path, Method::Get, move |request| {
        request
            .into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-store"),
                ],
            )?
            .write_all(get_current().to_string().as_bytes())
    })?;
    server.fn_handler::<EspIOError, _>(path, Method::Post, move |mut request| {
        let length = request
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok());
        if request.header("Content-Type") != Some("application/json")
            || !matches!(length, Some(1..=3))
        {
            return request
                .into_status_response(400)?
                .write_all(b"Invalid request");
        }
        let mut bytes = [0; 3];
        let length = length.unwrap();
        let mut offset = 0;
        while offset < length {
            let count = request.read(&mut bytes[offset..length])?;
            if count == 0 {
                return request
                    .into_status_response(400)?
                    .write_all(b"Incomplete request");
            }
            offset += count;
        }
        let value = crate::settings::parse_value(&bytes[..length], max);
        let Some(value) = value else {
            return request
                .into_status_response(400)?
                .write_all(b"Value out of range");
        };
        if value != get() {
            if let Err(err) = set(value) {
                log::error!("Failed to save setting: {err}");
                return request
                    .into_status_response(500)?
                    .write_all(b"Could not save setting");
            }
        }
        request.into_ok_response()?.write_all(b"Saved")
    })?;
    Ok(())
}
