//! Windows Media Foundation, the system's own decoders, as far as a video's
//! sound saved as MP3 (`mp3/decode/win.rs`) and a part saved as GIF
//! (`frames/win.rs`) need them: COM and Media Foundation started for the
//! length of one call and undone when it ends, a source reader over a sound
//! file, and the system's H.264 decoder. A Windows "N" edition has neither
//! the AAC nor the H.264 decoder until the Media Feature Pack is added; the
//! probes say so and the quality list then does not offer what cannot be
//! made.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;

use windows::core::{Interface, GUID, PCWSTR};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFSourceReader, IMFTransform, MFAudioFormat_AAC,
    MFCreateAttributes, MFCreateFile, MFCreateSourceReaderFromByteStream,
    MFCreateSourceReaderFromURL, MFMediaType_Audio, MFMediaType_Video, MFShutdown, MFStartup,
    MFTEnumEx, MFVideoFormat_H264, MFSTARTUP_FULL, MFT_CATEGORY_AUDIO_DECODER,
    MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_LOCALMFT, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_REGISTER_TYPE_INFO, MF_ACCESSMODE_READ, MF_BYTESTREAM_CONTENT_TYPE,
    MF_FILEFLAGS_NONE, MF_OPENMODE_FAIL_IF_NOT_EXIST, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};

/// The reader's names for "the first sound" and for every stream at once.
pub(crate) const FIRST_AUDIO: u32 = 0xFFFF_FFFD;
pub(crate) const ALL_STREAMS: u32 = 0xFFFF_FFFE;
/// `MF_SOURCE_READERF_ENDOFSTREAM`, in a read's stream flags.
pub(crate) const END_OF_STREAM: u32 = 0x2;

/// COM and Media Foundation, started on this thread for as long as this
/// lives. A thread that is already in another COM mode is used as it is.
pub(crate) struct Started {
    com: bool,
}

impl Started {
    pub(crate) fn new() -> Result<Started, String> {
        // SAFETY: no reserved pointer; balanced by CoUninitialize in Drop
        // only where it succeeded (S_OK and S_FALSE both count).
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        // SAFETY: no pointers; balanced by MFShutdown in Drop.
        if let Err(e) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            if com {
                // SAFETY: undoes the CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
            return Err(format!("the system's media library would not start ({e})"));
        }
        Ok(Started { com })
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        // SAFETY: undoes what `new` did, once.
        unsafe {
            let _ = MFShutdown();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

/// A source reader over the sound file at `path`. A file the reader cannot
/// tell the kind of by its name (a staged part with another extension) is
/// opened as a byte stream said to be an MP4.
pub(crate) fn reader(path: &Path) -> Result<IMFSourceReader, String> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let failed = |e: windows::core::Error| format!("the system's media library failed ({e})");
    let mut made: Option<IMFAttributes> = None;
    // SAFETY: `made` is a valid out pointer; the attribute set, the stream
    // and the reader are COM objects released when dropped; `wide` is a
    // NUL-terminated string that outlives every call that reads it.
    unsafe {
        MFCreateAttributes(&mut made, 1).map_err(failed)?;
        let attributes = made.as_ref().ok_or("the system's media library failed")?;
        if let Ok(reader) = MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), attributes) {
            return Ok(reader);
        }
        let stream = MFCreateFile(
            MF_ACCESSMODE_READ,
            MF_OPENMODE_FAIL_IF_NOT_EXIST,
            MF_FILEFLAGS_NONE,
            PCWSTR(wide.as_ptr()),
        )
        .map_err(|e| format!("{} could not be opened ({e})", path.display()))?;
        if let Ok(hint) = stream.cast::<IMFAttributes>() {
            let mime: Vec<u16> = "video/mp4"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let _ = hint.SetString(&MF_BYTESTREAM_CONTENT_TYPE, PCWSTR(mime.as_ptr()));
        }
        MFCreateSourceReaderFromByteStream(&stream, attributes)
            .map_err(|e| format!("the file is not one the system can read ({e})"))
    }
}

/// The system's own software decoders in `category` that take `subtype` of
/// `major`, best first.
pub(crate) fn decoders(category: GUID, major: GUID, subtype: GUID) -> Vec<IMFActivate> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: major,
        guidSubtype: subtype,
    };
    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    let mut found = Vec::new();
    // SAFETY: the out pointers are valid; each activation object in the
    // list is moved out (so it is released when dropped), then the list
    // itself, which COM allocated, is freed.
    unsafe {
        let ok = MFTEnumEx(
            category,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            None,
            &mut list,
            &mut count,
        )
        .is_ok();
        if !list.is_null() {
            if ok {
                for at in 0..count as usize {
                    if let Some(one) = std::ptr::read(list.add(at)) {
                        found.push(one);
                    }
                }
            }
            CoTaskMemFree(Some(list.cast()));
        }
    }
    found
}

/// Whether the system has a decoder for `subtype` of `major` in `category`.
pub(crate) fn decoder_present(category: GUID, major: GUID, subtype: GUID) -> bool {
    let Ok(_on) = Started::new() else {
        return false;
    };
    !decoders(category, major, subtype).is_empty()
}

/// Whether the system can decode AAC (an M4A's, an MP4's sound).
pub(crate) fn aac_decoder() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        decoder_present(
            MFT_CATEGORY_AUDIO_DECODER,
            MFMediaType_Audio,
            MFAudioFormat_AAC,
        )
    })
}

/// Whether the system can decode H.264 (a part's picture).
pub(crate) fn h264_decoder() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        decoder_present(
            MFT_CATEGORY_VIDEO_DECODER,
            MFMediaType_Video,
            MFVideoFormat_H264,
        )
    })
}

/// The system's H.264 decoder, ready to be given its input type.
pub(crate) fn h264_transform() -> Result<IMFTransform, String> {
    let first = decoders(
        MFT_CATEGORY_VIDEO_DECODER,
        MFMediaType_Video,
        MFVideoFormat_H264,
    )
    .into_iter()
    .next()
    .ok_or("the system has no H.264 decoder")?;
    // SAFETY: the activation object is live; the transform it makes is a
    // COM object released when dropped.
    unsafe { first.ActivateObject::<IMFTransform>() }
        .map_err(|e| format!("the system's video decoder would not start ({e})"))
}
