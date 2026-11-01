//! The sound of a file read by Media Foundation's source reader: the
//! system's decoder turns AAC (an M4A's, an MP4's sound; also an MP3) into
//! 16-bit samples, at most two channels, at a rate MP3 has. A Windows "N"
//! edition has no AAC decoder until the Media Feature Pack is added; the
//! read then fails in words and `mp3::possible` says no beforehand.

use std::path::Path;

use windows::Win32::Media::MediaFoundation::{
    IMFSample, MFAudioFormat_PCM, MFCreateMediaType, MFMediaType_Audio,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE,
    MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
};

use super::Pcm;
use crate::mf;

/// The rate MP3 has that a sound of `native` Hz is made at: 32, 44.1 or 48
/// kHz; the reader's resampler does the change.
fn mp3_rate(native: u32) -> u32 {
    match native {
        0..=32000 => 32000,
        32001..=44100 => 44100,
        _ => 48000,
    }
}

pub(super) fn read(path: &Path) -> Result<Pcm, String> {
    let _on = mf::Started::new()?;
    let sorry = |e: windows::core::Error| format!("the sound could not be read ({e})");
    let none = || "the file has no sound to save as MP3".to_string();
    let reader = mf::reader(path)?;
    // SAFETY: every COM object here is released when dropped; the out
    // pointers are valid for each call; a sample's buffer is read only
    // between its Lock and Unlock, within the length the buffer reports.
    unsafe {
        reader
            .SetStreamSelection(mf::ALL_STREAMS, false)
            .map_err(sorry)?;
        reader
            .SetStreamSelection(mf::FIRST_AUDIO, true)
            .map_err(|_| none())?;
        let native = reader
            .GetNativeMediaType(mf::FIRST_AUDIO, 0)
            .map_err(|_| none())?;
        let channels = native
            .GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)
            .unwrap_or(2)
            .clamp(1, 2);
        let rate = mp3_rate(
            native
                .GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)
                .unwrap_or(44100),
        );
        let wanted = MFCreateMediaType().map_err(sorry)?;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(sorry)?;
        wanted
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
            .map_err(sorry)?;
        for (key, value) in [
            (&MF_MT_AUDIO_NUM_CHANNELS, channels),
            (&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate),
            (&MF_MT_AUDIO_BITS_PER_SAMPLE, 16),
            (&MF_MT_AUDIO_BLOCK_ALIGNMENT, channels * 2),
            (&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * channels * 2),
            (&MF_MT_ALL_SAMPLES_INDEPENDENT, 1),
        ] {
            wanted.SetUINT32(key, value).map_err(sorry)?;
        }
        reader
            .SetCurrentMediaType(mf::FIRST_AUDIO, None, &wanted)
            .map_err(|e| format!("the system's sound decoder would not give 16-bit sound ({e})"))?;
        // What the decoder settled on, which is what the samples are.
        let given = reader.GetCurrentMediaType(mf::FIRST_AUDIO).map_err(sorry)?;
        let channels = given
            .GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)
            .unwrap_or(channels)
            .clamp(1, 2);
        let rate = given
            .GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)
            .unwrap_or(rate);
        if !matches!(rate, 32000 | 44100 | 48000) {
            return Err(format!("MP3 is made at 32, 44.1 or 48 kHz, not {rate} Hz"));
        }

        let mut samples: Vec<i16> = Vec::new();
        let mut empty = 0u32;
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            reader
                .ReadSample(
                    mf::FIRST_AUDIO,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
                .map_err(sorry)?;
            if let Some(sample) = sample {
                empty = 0;
                let buffer = sample.ConvertToContiguousBuffer().map_err(sorry)?;
                let mut data = std::ptr::null_mut::<u8>();
                let mut length = 0u32;
                buffer
                    .Lock(&mut data, None, Some(&mut length))
                    .map_err(sorry)?;
                if !data.is_null() {
                    let bytes = std::slice::from_raw_parts(data, length as usize);
                    samples.extend(
                        bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|b| i16::from_le_bytes(*b)),
                    );
                }
                buffer.Unlock().map_err(sorry)?;
            } else {
                empty += 1;
            }
            // Reads with no sound that are not the end (stream ticks) are
            // waited out, but not for ever.
            if flags & mf::END_OF_STREAM != 0 || empty > 1000 {
                break;
            }
        }
        Ok(Pcm {
            samples,
            channels: channels as usize,
            rate,
        })
    }
}
