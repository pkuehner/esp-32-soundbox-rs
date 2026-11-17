use crate::wav::WaveHeader;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};

pub fn valid_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else {
        return false;
    };
    (1..=8).contains(&stem.len())
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        && ext.eq_ignore_ascii_case("wav")
}

pub fn is_temporary_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else {
        return false;
    };
    stem.len() == 8
        && stem.bytes().all(|byte| byte.is_ascii_hexdigit())
        && ext.eq_ignore_ascii_case("tmp")
}

pub fn json_string(text: &str) -> String {
    let mut result = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\u{0}'..='\u{1f}' => result.push_str(&format!("\\u{:04x}", ch as u32)),
            _ => result.push(ch),
        }
    }
    result.push('"');
    result
}

pub fn valid_existing_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch.is_control())
}

pub fn audio_enabled(name: &str) -> Option<bool> {
    let (_, ext) = name.rsplit_once('.')?;
    if ext.eq_ignore_ascii_case("wav") {
        Some(true)
    } else if ext.eq_ignore_ascii_case("wad") {
        Some(false)
    } else {
        None
    }
}

pub fn toggle_audio(directory: &Path, name: &str) -> io::Result<()> {
    if !valid_existing_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid filename",
        ));
    }
    let enabled = audio_enabled(name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Not a WAV or WAD file"))?;
    let (stem, _) = name.rsplit_once('.').unwrap();
    if stem.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid filename",
        ));
    }
    let destination = format!("{stem}.{}", if enabled { "wad" } else { "wav" });
    let mut source = None;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let entry_name = entry.file_name();
        if entry_name == name {
            if !is_regular_file(&entry)? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Not a regular file",
                ));
            }
            source = Some(entry.path());
        }
        if entry_name
            .to_string_lossy()
            .eq_ignore_ascii_case(&destination)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination exists",
            ));
        }
    }
    let source = source.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not found"))?;
    let destination = directory.join(destination);
    match fs::metadata(&destination) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination exists",
            ))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    fs::rename(source, destination)
}

pub fn is_regular_file(entry: &fs::DirEntry) -> io::Result<bool> {
    // Rust's ESP-IDF libc in this toolchain uses incompatible directory type values.
    #[cfg(target_os = "espidf")]
    {
        Ok(fs::metadata(entry.path())?.is_file())
    }
    #[cfg(not(target_os = "espidf"))]
    {
        Ok(entry.file_type()?.is_file())
    }
}

pub fn list_files(directory: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if is_regular_file(&entry)? {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Non-UTF-8 filename"))?;
            names.push(name);
        }
    }
    names.sort_unstable();
    Ok(names)
}

pub fn delete(directory: &Path, name: &str) -> io::Result<()> {
    if !valid_existing_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid filename",
        ));
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name() == name {
            if !is_regular_file(&entry)? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Not a regular file",
                ));
            }
            return fs::remove_file(entry.path());
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "File not found"))
}

struct TemporaryFile {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Err(err) = fs::remove_file(&self.path) {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!("Failed to remove unfinished upload {:?}: {err}", self.path);
            }
        }
    }
}

pub fn receive(
    directory: &Path,
    name: &str,
    length: u64,
    mut read: impl FnMut(&mut [u8]) -> io::Result<usize>,
    mut random: impl FnMut() -> u32,
) -> io::Result<()> {
    if !valid_name(name) || length == 0 || length > u64::from(u32::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid filename or file size",
        ));
    }
    let destination = directory.join(name);
    match fs::metadata(&destination) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "File already exists",
            ))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => (),
        Err(err) => return Err(err),
    }
    let mut temp = None;
    for _ in 0..16 {
        let path = directory.join(format!("{:08X}.TMP", random()));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                temp = Some(TemporaryFile {
                    path,
                    file: Some(file),
                });
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    let mut temp = temp.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Could not create temporary file",
        )
    })?;
    let file = temp.file.as_mut().expect("temporary file is open");
    let mut remaining = length;
    let mut buffer = [0; 4096];
    while remaining > 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        let count = read(&mut buffer[..limit])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Upload interrupted",
            ));
        }
        file.write_all(&buffer[..count])?;
        remaining -= count as u64;
    }
    file.sync_all()?;
    drop(temp.file.take());
    // Validate with a read-only handle: FAT seeks on writable files can extend them.
    let mut file = File::open(&temp.path)?;
    let header = WaveHeader::read(&mut file).map_err(|err| match err.kind() {
        io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData => {
            io::Error::new(io::ErrorKind::InvalidData, "Expected a mono PCM WAV file")
        }
        _ => err,
    })?;
    let data_start = file.stream_position()?;
    if header.data_len == 0
        || header.data_len % u32::from(header.bits_per_sample / 8) != 0
        || data_start + u64::from(header.data_len) > length
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Truncated or incomplete WAV audio",
        ));
    }
    drop(file);
    fs::rename(&temp.path, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    fn wav(channels: u8) -> Vec<u8> {
        let mut bytes = b"RIFF\x26\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x44\xac\x00\x00\x88\x58\x01\x00\x02\x00\x10\x00data\x02\x00\x00\x00\x11\x22".to_vec();
        bytes[22] = channels;
        bytes
    }

    #[test]
    fn names_and_json_are_safe() {
        for name in ["a.wav", "A1234_-8.WAV"] {
            assert!(valid_name(name));
        }
        for name in [
            "",
            "../x.wav",
            "longname9.wav",
            "a.wav.tmp",
            ".wav",
            "a/b.wav",
            "a b.wav",
        ] {
            assert!(!valid_name(name));
        }
        for name in ["", ".", "..", "../x", "a/b", "a\\b", "a\n"] {
            assert!(!valid_existing_name(name));
        }
        assert!(valid_existing_name("LONGNA~1.WAV"));
        assert!(is_temporary_name("ABC01234.TMP"));
        assert!(is_temporary_name("abc01234.tmp"));
        assert!(!is_temporary_name("sounds.tmp"));
        assert_eq!(json_string("a\"\\\n"), "\"a\\\"\\\\\\u000a\"");
    }

    #[test]
    fn toggles_audio_without_overwriting_other_files() {
        let dir = std::env::temp_dir().join(format!("soundbox-toggle-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("song.WAV"), b"audio").unwrap();
        assert_eq!(audio_enabled("song.WAV"), Some(true));
        toggle_audio(&dir, "song.WAV").unwrap();
        assert_eq!(audio_enabled("song.wad"), Some(false));
        assert_eq!(audio_enabled("song.WAD"), Some(false));
        assert_eq!(fs::read(dir.join("song.wad")).unwrap(), b"audio");
        fs::write(dir.join("SONG.WAV"), b"other").unwrap();
        assert_eq!(
            toggle_audio(&dir, "song.wad").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(dir.join("SONG.WAV")).unwrap(), b"other");
        assert_eq!(fs::read(dir.join("song.wad")).unwrap(), b"audio");
        fs::remove_file(dir.join("SONG.WAV")).unwrap();
        toggle_audio(&dir, "song.wad").unwrap();
        assert_eq!(fs::read(dir.join("song.wav")).unwrap(), b"audio");
        assert_eq!(audio_enabled("song.mp3"), None);
        for name in ["../song.wav", "song.mp3", ".wav"] {
            assert_eq!(
                toggle_audio(&dir, name).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(
            toggle_audio(&dir, "missing.wav").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::create_dir(dir.join("nested.wav")).unwrap();
        assert_eq!(
            toggle_audio(&dir, "nested.wav").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        fs::remove_dir(dir.join("nested.wav")).unwrap();
        fs::remove_file(dir.join("song.wav")).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn validates_upload_and_never_overwrites_or_leaves_partial_files() {
        let dir = std::env::temp_dir().join(format!("soundbox-upload-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let data = wav(1);
        let mut input = Cursor::new(&data);
        receive(
            &dir,
            "test.wav",
            data.len() as u64,
            |buf| input.read(buf),
            || 1,
        )
        .unwrap();
        assert_eq!(fs::read(dir.join("test.wav")).unwrap(), data);
        let mut input = Cursor::new(wav(1));
        assert_eq!(
            receive(
                &dir,
                "test.wav",
                data.len() as u64,
                |buf| input.read(buf),
                || 2
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::AlreadyExists
        );
        let mut partial_sample = data.clone();
        partial_sample[40] = 1;
        for bad in [wav(2), data[..data.len() - 1].to_vec(), partial_sample] {
            let mut input = Cursor::new(&bad);
            assert!(receive(
                &dir,
                "bad.wav",
                bad.len() as u64,
                |buf| input.read(buf),
                || 3
            )
            .is_err());
        }
        let mut input = Cursor::new(&data[..10]);
        assert!(receive(
            &dir,
            "bad.wav",
            data.len() as u64,
            |buf| input.read(buf),
            || 4
        )
        .is_err());
        assert!(receive(
            &dir,
            "bad.wav",
            data.len() as u64,
            |_| Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "disconnected"
            )),
            || 5
        )
        .is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert!(delete(&dir, "../test.wav").is_err());
        fs::create_dir(dir.join("nested")).unwrap();
        assert_eq!(list_files(&dir).unwrap(), vec!["test.wav"]);
        assert!(delete(&dir, "nested").is_err());
        fs::remove_dir(dir.join("nested")).unwrap();
        delete(&dir, "test.wav").unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
