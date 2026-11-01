//! An AAC-LC decoder of dm's own, for the computers whose system has none that dm reads
//! (Linux), so a video's sound can be saved as MP3 there too.
//!
//! What is here, in the standard's order (ISO/IEC 14496-3, clean-room, from the standard alone):
//! the AudioSpecificConfig out of an M4A's `esds`, the frame's syntax (`decoder.rs`: sections,
//! scale factors, pulses, mid/side, TNS), the inverse MDCT with its windows and overlap-add
//! (`transform.rs`, tested against the defining sum), and the M4A's own sample tables and edit
//! list for where the sound starts and ends (read by the MP4 muxer's reader).
//!
//! What is not: the Huffman books and most band layouts (`tables.rs`). They are several hundred
//! code words of the standard's data, and a wrong one decodes to noise without any error, so
//! none were written from memory. Until they are supplied and checked, `decoder_ready` is false
//! and the MP3 choice is not offered where the system has no decoder: the same answer as before
//! this module, not a wrong one. Sound with SBR/PS (HE-AAC), more than two channels, intensity
//! stereo and noise substitution are refused the same way, with the reason.

mod bits;
mod decoder;
mod huff;
mod tables;
mod transform;

use crate::mp3::decode::Pcm;
use crate::mp4mux::{tracks_in_file, Track};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Whether this decoder can decode: every table the standard needs is in.
pub fn decoder_ready() -> bool {
    tables::COMPLETE
}

/// What an `esds` says about the sound.
#[derive(Debug, PartialEq, Eq)]
struct Config {
    rate_index: usize,
    rate: u32,
    channels: usize,
}

/// The AudioSpecificConfig of AAC-LC, 2 bytes or more.
fn parse_config(bytes: &[u8]) -> Result<Config, String> {
    let mut bits = bits::Bits::new(bytes);
    let mut object = bits.bits(5)?;
    if object == 31 {
        object = 32 + bits.bits(6)?;
    }
    if object != 2 {
        return Err(format!(
            "AAC object type {object} (only AAC-LC is converted to MP3)"
        ));
    }
    let rate_index = bits.bits(4)? as usize;
    let rate = if rate_index == 15 {
        bits.bits(24)?
    } else {
        *tables::RATES
            .get(rate_index)
            .ok_or("a sample rate AAC does not have")?
    };
    let channels = bits.bits(4)? as usize;
    if bits.flag()? {
        return Err("AAC with 960-sample frames is not supported".into());
    }
    if channels == 0 || channels > 2 {
        return Err("only mono and stereo sound is converted to MP3".into());
    }
    Ok(Config {
        rate_index,
        rate,
        channels,
    })
}

/// Walks the descriptors of an `esds` payload for the decoder-specific info.
fn descriptor_info(mut data: &[u8]) -> Option<&[u8]> {
    // The full box's version and flags.
    data = data.get(4..)?;
    loop {
        let tag = *data.first()?;
        let mut at = 1;
        let mut length = 0usize;
        for _ in 0..4 {
            let byte = *data.get(at)?;
            at += 1;
            length = (length << 7) | usize::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                break;
            }
        }
        let body = data.get(at..at.checked_add(length)?)?;
        match tag {
            0x03 => data = body.get(3..)?,
            0x04 => data = body.get(13..)?,
            0x05 => return Some(body),
            _ => data = data.get(at + length..)?,
        }
    }
}

/// The audio sample entry's `esds` payload and the entry's codec name in a whole `stsd` box.
fn sample_entry(stsd: &[u8]) -> Option<([u8; 4], &[u8])> {
    let entry = stsd.get(16..)?;
    let name: [u8; 4] = entry.get(4..8)?.try_into().ok()?;
    // 8 bytes of box header, 28 of audio sample entry, then child boxes.
    let mut children = entry.get(36..)?;
    while children.len() >= 8 {
        let size = u32::from_be_bytes(children.get(0..4)?.try_into().ok()?) as usize;
        let kind = children.get(4..8)?;
        if size < 8 || size > children.len() {
            return None;
        }
        if kind == b"esds" {
            return Some((name, children.get(8..size)?));
        }
        children = children.get(size..)?;
    }
    Some((name, &[]))
}

fn audio_track(path: &Path) -> Result<(Track, Config), String> {
    let tracks = tracks_in_file(path)?;
    let track = tracks
        .into_iter()
        .find(|t| t.hdlr.get(16..20) == Some(b"soun"))
        .ok_or("the file has no sound track")?;
    let (name, esds) = sample_entry(&track.stsd).ok_or("the sound track's description is unreadable")?;
    if &name != b"mp4a" {
        return Err(format!(
            "sound coded as {}, not AAC",
            String::from_utf8_lossy(&name)
        ));
    }
    let info = descriptor_info(esds).ok_or("an AAC track with no decoder configuration")?;
    let config = parse_config(info)?;
    Ok((track, config))
}

/// `path`'s sound (an M4A, or an MP4's sound track) as 16-bit samples.
pub fn read(path: &Path) -> Result<Pcm, String> {
    let (track, config) = audio_track(path)?;
    let mut decoder = decoder::Decoder::new(config.rate_index, config.channels)?;
    let mut file = File::open(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    let mut samples: Vec<i16> = Vec::new();
    for sample in &track.samples {
        let mut data = vec![0u8; sample.size as usize];
        file.seek(SeekFrom::Start(sample.offset))
            .and_then(|_| file.read_exact(&mut data))
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        let channels = decoder.frame(&data)?;
        for i in 0..1024 {
            for channel in &channels {
                let value = channel.get(i).copied().unwrap_or(0.0);
                samples.push(value.round().clamp(-32768.0, 32767.0) as i16);
            }
        }
    }
    // The edit list drops the encoder's priming samples and any padding at the end.
    let per_frame = config.channels;
    if let Some(edit) = track.edits.first() {
        let skip = usize::try_from(edit.media_time.max(0)).unwrap_or(0) * per_frame;
        let keep = if edit.duration > 0 && track.edits_timescale > 0 {
            (edit.duration.saturating_mul(u64::from(config.rate)) / u64::from(track.edits_timescale))
                as usize
                * per_frame
        } else {
            usize::MAX
        };
        let start = skip.min(samples.len());
        let end = start.saturating_add(keep).min(samples.len());
        samples = samples.get(start..end).unwrap_or(&[]).to_vec();
    }
    Ok(Pcm {
        samples,
        channels: config.channels,
        rate: config.rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stereo_lc_config_at_44100_is_understood() {
        // 00010 0100 0010 000: object 2, rate index 4, 2 channels, 1024-frame, no core, no ext.
        let config = parse_config(&[0x12, 0x10]).unwrap();
        assert_eq!(
            config,
            Config {
                rate_index: 4,
                rate: 44100,
                channels: 2
            }
        );
    }

    #[test]
    fn other_object_types_and_channel_counts_are_refused_with_a_reason() {
        // HE-AAC (object 5), 6 channels (config 6).
        assert!(parse_config(&[0x2b, 0x11]).unwrap_err().contains("only AAC-LC"));
        assert!(parse_config(&[0x11, 0xb0]).unwrap_err().contains("mono and stereo"));
        assert!(parse_config(&[0x12]).is_err());
    }

    #[test]
    fn the_decoder_specific_info_is_found_in_an_esds() {
        let esds: Vec<u8> = [
            &[0u8, 0, 0, 0][..],               // version and flags
            &[0x03, 0x16, 0, 1, 0],            // ES_Descr, id 1, no flags
            &[0x04, 0x11, 0x40, 0x15, 0, 0, 0], // DecoderConfig: AAC, audio stream
            &[0, 0, 0, 0, 0, 0, 0, 0],         // max and average bit rate
            &[0x05, 0x02, 0x12, 0x10],         // DecSpecificInfo
        ]
        .concat();
        assert_eq!(descriptor_info(&esds), Some(&[0x12u8, 0x10][..]));
        assert_eq!(descriptor_info(&esds[..9]), None);
    }

    #[test]
    fn a_file_that_is_not_an_mp4_is_refused_not_decoded() {
        let dir = std::env::temp_dir().join(format!("dm-aac-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("noise.m4a");
        std::fs::write(&path, b"not an mp4 at all").unwrap();
        assert!(read(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_fixture_m4a_is_read_up_to_the_missing_tables() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mp4/sound.f140.m4a");
        let (track, config) = audio_track(&fixture).unwrap();
        assert_eq!(config.channels, 2);
        assert!(config.rate >= 8000);
        assert!(!track.samples.is_empty());
        // With the tables absent the decode itself says so, instead of returning noise.
        let error = read(&fixture).unwrap_err();
        assert!(error.contains("Huffman"), "{error}");
        assert!(!decoder_ready());
    }
}
