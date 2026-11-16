use std::io::{self, Read, Seek, SeekFrom};

#[derive(Debug)]
pub struct WaveHeader {
    pub num_channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub data_len: u32,
}

impl WaveHeader {
    pub fn read(file: &mut (impl Read + Seek)) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Unsupported WAV file");
        let mut riff = [0; 12];
        file.read_exact(&mut riff)?;
        if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
            return Err(invalid());
        }

        let mut format = None;
        loop {
            let mut chunk = [0; 8];
            file.read_exact(&mut chunk)?;
            let size = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
            if &chunk[..4] == b"fmt " {
                if size < 16 {
                    return Err(invalid());
                }
                let mut fmt = [0; 16];
                file.read_exact(&mut fmt)?;
                let encoding = u16::from_le_bytes(fmt[0..2].try_into().unwrap());
                let channels = u16::from_le_bytes(fmt[2..4].try_into().unwrap());
                let sample_rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
                let bits = u16::from_le_bytes(fmt[14..16].try_into().unwrap());
                if encoding != 1
                    || channels != 1
                    || sample_rate == 0
                    || ![8, 16, 24, 32].contains(&bits)
                {
                    return Err(invalid());
                }
                format = Some((channels, sample_rate, bits));
                file.seek(SeekFrom::Current(
                    i64::from(size - 16) + i64::from(size % 2),
                ))?;
            } else if &chunk[..4] == b"data" {
                let (num_channels, sample_rate, bits_per_sample) = format.ok_or_else(invalid)?;
                return Ok(Self {
                    num_channels,
                    sample_rate,
                    bits_per_sample,
                    data_len: size,
                });
            } else {
                file.seek(SeekFrom::Current(i64::from(size) + i64::from(size % 2)))?;
            }
        }
    }
}

#[cfg(test)]
#[allow(dead_code, unused_imports)]
mod tests {
    use super::WaveHeader;
    use std::io::{Cursor, Read};

    #[test]
    fn skips_metadata_and_stops_at_audio() {
        let mut bytes = b"RIFF\x00\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x44\xac\x00\x00\x88\x58\x01\x00\x02\x00\x10\x00".to_vec();
        bytes.extend_from_slice(b"LIST\x03\x00\x00\x00abc\x00data\x02\x00\x00\x00\x11\x22");
        let mut file = Cursor::new(bytes);
        let header = WaveHeader::read(&mut file).unwrap();
        assert_eq!(
            (
                header.num_channels,
                header.sample_rate,
                header.bits_per_sample,
                header.data_len
            ),
            (1, 44_100, 16, 2)
        );
        let mut audio = [0; 2];
        file.read_exact(&mut audio).unwrap();
        assert_eq!(audio, [0x11, 0x22]);
    }

    #[test]
    fn rejects_non_wav_and_stereo() {
        assert!(WaveHeader::read(&mut Cursor::new(b"not a wave file")).is_err());
        let bytes = b"RIFF\x00\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x02\x00\x44\xac\x00\x00\x10\xb1\x02\x00\x04\x00\x10\x00data\x00\x00\x00\x00";
        assert!(WaveHeader::read(&mut Cursor::new(bytes)).is_err());
    }
}
